//! Shape and parameter optimization of a CalculiX model: a Nelder-Mead loop around gmsh and
//! `ccx`, in the process of `fh`.
//!
//! An optimization case is a folder with `optimize.toml`, `mesh.geo` and `job.inp`. `mesh.geo`
//! is a gmsh input that makes the geometry and the mesh from the design variables. It defines
//! each variable with `DefineConstant[ name = value ];`, so that `-setnumber` can change it.
//! `job.inp` is a CalculiX model that includes the mesh (`*INCLUDE, INPUT=mesh.inp`).
//! `optimize.toml` holds these keys:
//!
//! - `field`: a field of the `.frd` file with 1 value at each node, for example `MISES` (from
//!   `*EL FILE` with `S`);
//! - `set`: a node set of the model;
//! - `[variables]`: for each variable of `mesh.geo`, `name = { start = S, low = L, high = H }`.
//!
//! The loop minimizes the largest value of `field` at the nodes of `set` in the last step, with
//! the Nelder-Mead method in the box of the variables. For each design, gmsh writes `mesh.inp`
//! (`gmsh mesh.geo -setnumber NAME VALUE ... -3 -format inp -o mesh.inp`), and `ccx` solves
//! `job.inp` (see [`crate::ccx`]). The first simplex is the start and, for each variable, the
//! start with a step of 0.1 (high - low) in that variable. The loop stops when each point of
//! the simplex is within 0.001 (high - low) of the best point in each variable, or after 500
//! solves. gmsh and `ccx` stay separate programs.
//!
//! The run writes `<run-dir>/optimize/history.txt`, with 1 row for each solve: the number of the
//! solve, the variables and the objective. `optimum.txt` holds the row of the best design. Then
//! the run solves the best design again, with `OUTPUT=3D` on each `*NODE FILE` and `*EL FILE`,
//! so that the page draws 2D elements, and writes its fields to `step-N.vtu` and `results.pvd`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use fh_core::inp::{flatten, parse};
use toml_edit::{Document, TableLike};

use crate::ccx::{frd, prepare_run_dir, supervise, write_frames, CancelToken, Ccx, Program};
use crate::getdp;

/// The folder of the files of the loop in a run folder.
pub const WORK: &str = "optimize";
/// The table of the solves in [`WORK`].
pub const HISTORY: &str = "history.txt";
/// The table of the best design in [`WORK`].
pub const OPTIMUM: &str = "optimum.txt";
const SETTINGS: &str = "optimize.toml";

/// The first step of each variable, as a fraction of high - low.
const STEP: f64 = 0.1;
/// The loop stops when the simplex is this small, as a fraction of high - low.
const TOLERANCE: f64 = 1e-3;
/// The loop stops after this number of solves.
const SOLVES: usize = 500;

/// True when `dir` is an optimization case: it has `optimize.toml`.
pub fn is_case(dir: &Path) -> bool {
    dir.join(SETTINGS).is_file()
}

/// A design variable of `optimize.toml`.
#[derive(Debug, PartialEq)]
struct Variable {
    name: String,
    start: f64,
    low: f64,
    high: f64,
}

/// The keys of `optimize.toml`.
#[derive(Debug, PartialEq)]
struct Settings {
    field: String,
    set: String,
    variables: Vec<Variable>,
}

/// True when `name` is a variable name of gmsh: a letter or `_` first, then letters, digits
/// and `_`.
fn identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn settings(text: &str) -> Result<Settings, String> {
    let doc = Document::parse(text).map_err(|e| format!("{SETTINGS}: {}", e.message()))?;
    let table: &dyn TableLike = doc.as_table();
    if let Some((key, _)) = (table.iter()).find(|(k, _)| !["field", "set", "variables"].contains(k))
    {
        return Err(format!("{SETTINGS}: unknown key `{key}`"));
    }
    let text = |key: &str| {
        table
            .get(key)
            .ok_or_else(|| format!("{SETTINGS}: missing key `{key}`"))?
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("{SETTINGS}: `{key}` must be a text"))
    };
    let (field, set) = (text("field")?, text("set")?);
    let variables = table
        .get("variables")
        .and_then(|v| v.as_table_like())
        .ok_or_else(|| format!("{SETTINGS}: missing table `[variables]`"))?;
    let mut out = Vec::new();
    for (name, item) in variables.iter() {
        let bad = |why: &str| format!("{SETTINGS}: variable `{name}` {why}");
        if !identifier(name) {
            return Err(bad("needs a name of letters, digits and _"));
        }
        let keys = item
            .as_table_like()
            .ok_or_else(|| bad("must be a table { start = S, low = L, high = H }"))?;
        if let Some((key, _)) = keys
            .iter()
            .find(|(k, _)| !["start", "low", "high"].contains(k))
        {
            return Err(bad(&format!("has the unknown key `{key}`")));
        }
        let number = |key: &str| {
            let value = keys
                .get(key)
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)));
            value
                .filter(|v| v.is_finite())
                .ok_or_else(|| bad(&format!("needs the number `{key}`")))
        };
        let v = Variable {
            name: name.to_owned(),
            start: number("start")?,
            low: number("low")?,
            high: number("high")?,
        };
        if !(v.low < v.high && v.low <= v.start && v.start <= v.high) {
            return Err(bad("needs low < high and low <= start <= high"));
        }
        out.push(v);
    }
    if out.is_empty() {
        return Err(format!("{SETTINGS}: `[variables]` has no variable"));
    }
    Ok(Settings {
        field,
        set,
        variables: out,
    })
}

/// The keyword of an input line in upper case with no spaces, for example `*NODEFILE`.
fn keyword(line: &str) -> String {
    let word = line.split(',').next().unwrap_or_default();
    word.to_ascii_uppercase().replace(' ', "")
}

/// `text` with `OUTPUT=3D` on each `*NODE FILE` and `*EL FILE` that has no `OUTPUT`: `ccx` then
/// writes 2D elements as the 3D elements that the page draws.
fn output_3d(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let word = keyword(line);
        if (word == "*NODEFILE" || word == "*ELFILE")
            && !line.to_ascii_uppercase().contains("OUTPUT")
        {
            out.push_str(line.trim_end().trim_end_matches(','));
            out.push_str(", OUTPUT=3D\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The largest value of the field `name` (1 value at each node) at the nodes `ids`, in the last
/// frame of the `.frd` file.
fn largest(results: &frd::Frd, name: &str, set: &str, ids: &[u32]) -> Result<f64, String> {
    let frame = results.frames.last().ok_or("job.frd has no result")?;
    let field = frame.field(name).ok_or_else(|| {
        format!("job.frd has no field {name}. Add *EL FILE with S (for MISES) to the step")
    })?;
    let values = field
        .as_f64()
        .filter(|_| field.components == 1)
        .ok_or_else(|| {
            format!("the field {name} has more than 1 value at each node: use for example MISES")
        })?;
    let index: HashMap<u32, usize> = (results.nodes.iter().enumerate())
        .map(|(i, id)| (*id, i))
        .collect();
    let mut most = f64::NEG_INFINITY;
    for id in ids {
        let at = *index.get(id).ok_or_else(|| {
            format!(
                "job.frd has no value at node {id} of set {set}. Remove OUTPUT=3D from job.inp: \
                 the loop adds it to the last solve only"
            )
        })?;
        most = most.max(values[at]);
    }
    if most.is_finite() {
        Ok(most)
    } else {
        Err(format!(
            "set {set} has no node or no finite value of {name}"
        ))
    }
}

/// Minimize `f` in the box [0, 1]^n with the Nelder-Mead method from `start`, and give the best
/// point. The point `k + 1` of the first simplex moves variable `k` by [`STEP`]. When the move
/// does not change the value, variable `k` has no effect: the error names it.
fn minimize(
    names: &[String],
    start: &[f64],
    f: &mut dyn FnMut(&[f64]) -> Result<f64, String>,
) -> Result<Vec<f64>, String> {
    let n = start.len();
    let mut simplex = vec![(start.to_vec(), f(start)?)];
    for k in 0..n {
        let mut u = start.to_vec();
        u[k] += if u[k] + STEP <= 1.0 { STEP } else { -STEP };
        let value = f(&u)?;
        if value == simplex[0].1 {
            return Err(format!(
                "the variable {} does not change the objective. Define it in mesh.geo with \
                 DefineConstant[ {0} = ... ];, so that gmsh takes its value, and use it in the \
                 geometry",
                names[k]
            ));
        }
        simplex.push((u, value));
    }
    let mut calls = n + 1;
    loop {
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
        let best = &simplex[0].0;
        let size = (simplex[1..].iter())
            .flat_map(|(u, _)| u.iter().zip(best).map(|(a, b)| (a - b).abs()))
            .fold(0.0, f64::max);
        if size <= TOLERANCE || calls >= SOLVES {
            return Ok(simplex.swap_remove(0).0);
        }
        // The centre of all points but the worst, and a point on the line from the worst point
        // through the centre.
        let centre: Vec<f64> = (0..n)
            .map(|k| simplex[..n].iter().map(|(u, _)| u[k]).sum::<f64>() / n as f64)
            .collect();
        let (worst, f_worst) = simplex[n].clone();
        let point = |t: f64| -> Vec<f64> {
            (centre.iter().zip(&worst))
                .map(|(c, w)| (c + t * (w - c)).clamp(0.0, 1.0))
                .collect()
        };
        let reflected = point(-1.0);
        let f_reflected = f(&reflected)?;
        calls += 1;
        if f_reflected < simplex[0].1 {
            let expanded = point(-2.0);
            let f_expanded = f(&expanded)?;
            calls += 1;
            simplex[n] = if f_expanded < f_reflected {
                (expanded, f_expanded)
            } else {
                (reflected, f_reflected)
            };
        } else if f_reflected < simplex[n - 1].1 {
            simplex[n] = (reflected, f_reflected);
        } else {
            let contracted = point(if f_reflected < f_worst { -0.5 } else { 0.5 });
            let f_contracted = f(&contracted)?;
            calls += 1;
            if f_contracted < f_reflected.min(f_worst) {
                simplex[n] = (contracted, f_contracted);
            } else {
                // Shrink the simplex to the best point.
                let best = simplex[0].0.clone();
                for (u, value) in &mut simplex[1..] {
                    *u = best
                        .iter()
                        .zip(&*u)
                        .map(|(b, x)| b + 0.5 * (x - b))
                        .collect();
                    *value = f(u)?;
                    calls += 1;
                }
            }
        }
    }
}

/// One row of the history: the number of the solve, the value of each variable and the
/// objective.
#[derive(Debug, Clone)]
pub struct Solve {
    pub number: usize,
    pub variables: Vec<(String, f64)>,
    pub objective: f64,
}

/// The row of a solve in the tables.
fn row(s: &Solve) -> String {
    let values: Vec<String> = (s.variables.iter())
        .map(|(_, v)| format!("{v:.10e}"))
        .collect();
    format!("{} {} {:.10e}\n", s.number, values.join(" "), s.objective)
}

/// Optimize the case `case` in the folder `run_dir`, with `ccx` and its limits. The time limit
/// of `ccx` counts all the programs. The run clears the folder first, if a run made the folder
/// earlier. `on_solve` gets each row of the history.
pub fn run_with(
    ccx: &Ccx,
    case: &Path,
    run_dir: &Path,
    cancel: &CancelToken,
    on_solve: &mut dyn FnMut(&Solve),
) -> Result<(), String> {
    let start = Instant::now();
    let case =
        fs::canonicalize(case).map_err(|e| format!("{}: case folder: {e}", case.display()))?;
    let toml = case.join(SETTINGS);
    let text = fs::read_to_string(&toml).map_err(|e| format!("{}: {e}", toml.display()))?;
    let s = settings(&text)?;
    for file in ["mesh.geo", "job.inp"] {
        if !case.join(file).is_file() {
            return Err(format!("the optimization case has no {file}"));
        }
    }
    let gmsh_exe = crate::msh::find_gmsh()?;
    let run_dir = prepare_run_dir(run_dir, &case)?;
    let work = run_dir.join(WORK);
    getdp::copy_case(&case, &work, false)?;
    let folder = |e: std::io::Error| format!("run folder: {e}");
    let solve_dir = work.join("solve");
    let remaining = |ccx: &Ccx| {
        let mut each = ccx.clone();
        each.timeout = ccx.timeout.map(|t| t.saturating_sub(start.elapsed()));
        each
    };
    let timed_out = |e: String, solves: usize| match ccx.timeout {
        Some(t) if start.elapsed() >= t => format!(
            "the run stopped at the time limit of {} s after {solves} solves. Use a coarser mesh",
            t.as_secs_f64()
        ),
        _ => e,
    };
    // gmsh writes mesh.inp of the design `x`.
    let mesh = |x: &[f64]| -> Result<(), String> {
        let gmsh = Program {
            name: "gmsh",
            install: "Install gmsh (README.md) and put gmsh on PATH, or set FH_GMSH",
        };
        let mut args = vec!["mesh.geo".to_owned()];
        for (v, x) in s.variables.iter().zip(x) {
            args.extend(["-setnumber".to_owned(), v.name.clone(), format!("{x:.15e}")]);
        }
        args.extend(["-3", "-format", "inp", "-o", "mesh.inp"].map(str::to_owned));
        let log_file = work.join("log.gmsh");
        let log = fs::File::create(&log_file).map_err(folder)?;
        let mut cmd = Command::new(&gmsh_exe);
        cmd.args(&args)
            .current_dir(&work)
            // gmsh reads `~/.gmsh-options`. The run folder has none.
            .env("HOME", &work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(folder)?)
            .stderr(log);
        let limits = (start, ccx.timeout, ccx.memory_limit);
        let status = supervise(&gmsh, &mut cmd, limits, cancel, &mut || {})?;
        if !status.success() {
            let log = fs::read_to_string(&log_file).unwrap_or_default();
            return Err(getdp::failure(gmsh.name, status, &log, &log_file));
        }
        Ok(())
    };
    let real = |u: &[f64]| -> Vec<f64> {
        (s.variables.iter().zip(u))
            .map(|(v, u)| v.low + u * (v.high - v.low))
            .collect()
    };

    let mut history = format!(
        "# solve {} objective\n",
        (s.variables.iter())
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let mut solves: Vec<(Vec<f64>, f64)> = Vec::new();
    let mut best: Option<Solve> = None;
    let mut objective = |u: &[f64]| -> Result<f64, String> {
        // A point on a bound can come again: it needs no new solve.
        if let Some((_, value)) = solves.iter().find(|(p, _)| p == u) {
            return Ok(*value);
        }
        if cancel.is_cancelled() {
            return Err("run cancelled".into());
        }
        let x = real(u);
        mesh(&x).map_err(|e| timed_out(e, solves.len()))?;
        remaining(ccx)
            .run_with(&work.join("job.inp"), &solve_dir, cancel, &mut |_| {})
            .map_err(|e| timed_out(e, solves.len()))?;
        let model = flatten(&work.join("job.inp"))?;
        let ids = (parse(&model.text).nset(&s.set).map(|n| n.ids.clone()))
            .ok_or_else(|| format!("the model has no node set {}", s.set))?;
        let file = solve_dir.join("ccx/job.frd");
        let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let results = frd::read(&String::from_utf8_lossy(&bytes))?;
        let value = largest(&results, &s.field, &s.set, &ids)?;
        solves.push((u.to_vec(), value));
        let solve = Solve {
            number: solves.len(),
            variables: (s.variables.iter().map(|v| v.name.clone()))
                .zip(x)
                .collect(),
            objective: value,
        };
        history.push_str(&row(&solve));
        fs::write(work.join(HISTORY), &history).map_err(folder)?;
        on_solve(&solve);
        if best.as_ref().is_none_or(|b| value < b.objective) {
            best = Some(solve);
        }
        Ok(value)
    };
    let names: Vec<String> = s.variables.iter().map(|v| v.name.clone()).collect();
    let start_point: Vec<f64> = (s.variables.iter())
        .map(|v| (v.start - v.low) / (v.high - v.low))
        .collect();
    minimize(&names, &start_point, &mut objective)?;
    let best = best.expect("the loop solves the start");
    let header = history.lines().next().unwrap_or_default();
    fs::write(work.join(OPTIMUM), format!("{header}\n{}", row(&best))).map_err(folder)?;

    // The page shows the best design.
    let x: Vec<f64> = best.variables.iter().map(|(_, v)| *v).collect();
    mesh(&x).map_err(|e| timed_out(e, solves.len()))?;
    let flat = flatten(&work.join("job.inp"))?;
    fs::write(work.join("page.inp"), output_3d(&flat.text)).map_err(folder)?;
    remaining(ccx)
        .run_with(&work.join("page.inp"), &solve_dir, cancel, &mut |_| {})
        .map_err(|e| timed_out(e, solves.len()))?;
    let file = solve_dir.join("ccx/job.frd");
    let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let results = frd::read(&String::from_utf8_lossy(&bytes))?;
    let units = fh_core::inp::units(&flat.text);
    let units = match units.as_str() {
        "SI-mm" | "SI-m" => units.as_str(),
        _ => "unspecified",
    };
    let frames = results.frames.iter().map(|f| (f.time, results.grid(f)));
    write_frames(&run_dir, units, false, frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS_TEXT: &str = "field = \"MISES\"\nset = \"HOLE\"\n\n[variables]\n\
        ratio = { start = 1, low = 1, high = 4 }\npower = { start = 3.0, low = 1.6, high = 4.0 }\n";

    #[test]
    fn the_settings_need_a_field_a_set_and_variables_in_their_box() {
        let s = settings(SETTINGS_TEXT).unwrap();
        assert_eq!((s.field.as_str(), s.set.as_str()), ("MISES", "HOLE"));
        assert_eq!(
            s.variables,
            [
                Variable {
                    name: "ratio".into(),
                    start: 1.0,
                    low: 1.0,
                    high: 4.0
                },
                Variable {
                    name: "power".into(),
                    start: 3.0,
                    low: 1.6,
                    high: 4.0
                }
            ]
        );
        let table = SETTINGS_TEXT.replace("ratio = { start = 1, low = 1, high = 4 }\n", "")
            + "[variables.ratio]\nstart = 2\nlow = 1\nhigh = 4\n";
        assert_eq!(settings(&table).unwrap().variables[1].start, 2.0);
        let error = |text: String| settings(&text).unwrap_err();
        assert!(error(SETTINGS_TEXT.replace("set = \"HOLE\"\n", "")).contains("missing key `set`"));
        assert!(error(format!("op = \"max\"\n{SETTINGS_TEXT}")).contains("unknown key `op`"));
        assert!(error(SETTINGS_TEXT.replace("start = 1,", "start = 5,"))
            .contains("variable `ratio` needs low < high and low <= start <= high"));
        assert!(error(SETTINGS_TEXT.replace("low = 1.6,", "")).contains("needs the number `low`"));
        assert!(
            error(SETTINGS_TEXT.replace("high = 4 }", "high = 4, step = 1 }"))
                .contains("unknown key `step`")
        );
        assert!(error(SETTINGS_TEXT.replace("ratio =", "\"2a\" =")).contains("letters, digits"));
        let none = SETTINGS_TEXT[..SETTINGS_TEXT.find("ratio").unwrap()].to_owned();
        assert!(error(none).contains("has no variable"));
    }

    #[test]
    fn the_page_solve_gets_3d_output() {
        let text = "*Node File\nU\n*EL FILE, OUTPUT=2D\nS\n*EL FILE,\nE\n*NODE PRINT, NSET=A\nU\n";
        assert_eq!(
            output_3d(text),
            "*Node File, OUTPUT=3D\nU\n*EL FILE, OUTPUT=2D\nS\n*EL FILE, OUTPUT=3D\nE\n\
             *NODE PRINT, NSET=A\nU\n"
        );
    }

    /// The search finds the minimum of a function with a kink, as the largest stress of a hole
    /// has one at its optimum, and it stops in the tolerance.
    #[test]
    fn the_search_finds_the_minimum_of_a_kinked_function_in_the_box() {
        let names = ["a".to_owned(), "b".to_owned()];
        let mut calls = 0;
        let mut f = |u: &[f64]| {
            calls += 1;
            Ok((u[0] - 0.3).abs().max(2.0 * (u[1] - 0.6).abs()) + 0.1 * (u[0] - 0.3).powi(2))
        };
        let best = minimize(&names, &[0.9, 0.95], &mut f).unwrap();
        assert!(
            (best[0] - 0.3).abs() < 2e-3 && (best[1] - 0.6).abs() < 2e-3,
            "{best:?}"
        );
        assert!(calls < SOLVES, "{calls}");
        // The minimum on a bound: each point stays in the box.
        let mut f = |u: &[f64]| {
            assert!(u.iter().all(|v| (0.0..=1.0).contains(v)), "{u:?}");
            Ok(u[0] + (u[1] - 0.5).powi(2))
        };
        let best = minimize(&names, &[0.5, 0.95], &mut f).unwrap();
        assert!(best[0] < 2e-3 && (best[1] - 0.5).abs() < 0.05, "{best:?}");
    }

    #[test]
    fn a_variable_with_no_effect_is_an_error() {
        let names = ["ratio".to_owned(), "power".to_owned()];
        let mut f = |u: &[f64]| Ok(u[0]);
        let error = minimize(&names, &[0.0, 1.0], &mut f).unwrap_err();
        assert!(
            error.contains("the variable power does not change"),
            "{error}"
        );
        assert!(error.contains("DefineConstant[ power = ... ]"), "{error}");
    }
}
