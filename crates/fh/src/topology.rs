//! Topology optimization of a CalculiX model: a SIMP loop around `ccx`, in the process of `fh`.
//!
//! A topology case is a folder with `job.inp` and `topology.toml`. `job.inp` is a CalculiX model
//! with the mesh, the supports and 1 `*STATIC` step with the loads. It has no material and no
//! section: each element is a design element, and the loop writes its material and section.
//! `topology.toml` holds these keys:
//!
//! - `volume`: the fraction of the volume of the mesh that the material fills, above 0 and
//!   below 1;
//! - `radius`: the radius of the filter, in the length unit of the model;
//! - `young` and `poisson`: the elastic constants of the material;
//! - `thickness` (optional): the thickness of 2D elements. Without it, `ccx` uses 1.
//!
//! The loop is the method of Andreassen et al. (2011) with the sensitivity filter. The modulus
//! of an element with the density x is Emin + x^3 (young - Emin), with Emin = 1e-9 young. The
//! filter weight of 2 elements is max(0, radius - d), with d the distance of their centres. The
//! optimality criteria update moves a density by 0.2 at most. Each iteration solves the model
//! with `ccx` (see [`crate::ccx`]) and reads the strain energy (`ELSE`) and the volume (`EVOL`)
//! of each element in `job.dat`. The loop stops when no density changes by more than 0.01, or
//! after 500 iterations.
//!
//! The run writes `<run-dir>/topology/history.txt`, with 1 row for each iteration: the
//! iteration, the compliance and the volume fraction of the solved design, and the largest
//! change of a density after the solve. The last solve is in `<run-dir>/topology/solve`. Its
//! fields, with the density of each element as the cell field `density`, go to `step-N.vtu` and
//! `results.pvd`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use fh_core::inp::{flatten, parse};
use fh_core::vtu::Field;
use toml_edit::{Document, TableLike};

use crate::ccx::{frd, prepare_run_dir, stage_input, write_frames, CancelToken, Ccx};

/// The folder of the files of the loop in a run folder.
pub const WORK: &str = "topology";
/// The table of the iterations in [`WORK`].
pub const HISTORY: &str = "history.txt";
const SETTINGS: &str = "topology.toml";
/// The element set of all elements, in the input that the loop writes.
const SET: &str = "FHDESIGN";

const PENALTY: f64 = 3.0;
const MOVE: f64 = 0.2;
/// The loop stops when no density changes by more than this.
const CHANGE: f64 = 0.01;
const ITERATIONS: usize = 500;

/// True when `dir` is a topology case: it has `topology.toml`.
pub fn is_case(dir: &Path) -> bool {
    dir.join(SETTINGS).is_file()
}

/// Copy the topology case `case` into `dir`: `topology.toml`, and `job.inp` with its `*INCLUDE`
/// files. Each copy is read-only.
pub(crate) fn copy_case(case: &Path, dir: &Path) -> Result<(), String> {
    stage_input(&case.join("job.inp"), dir, "job.inp")?;
    let copy = || -> std::io::Result<()> {
        let dst = dir.join(SETTINGS);
        fs::copy(case.join(SETTINGS), &dst)?;
        let mut perms = fs::metadata(&dst)?.permissions();
        perms.set_readonly(true);
        fs::set_permissions(&dst, perms)
    };
    copy().map_err(|e| format!("case copy: {}: {e}", case.display()))
}

/// The keys of `topology.toml`.
#[derive(Debug, PartialEq)]
struct Settings {
    volume: f64,
    radius: f64,
    young: f64,
    poisson: f64,
    thickness: Option<f64>,
}

fn settings(text: &str) -> Result<Settings, String> {
    let doc = Document::parse(text).map_err(|e| format!("{SETTINGS}: {}", e.message()))?;
    let table: &dyn TableLike = doc.as_table();
    let keys = ["volume", "radius", "young", "poisson", "thickness"];
    if let Some((key, _)) = table.iter().find(|(k, _)| !keys.contains(k)) {
        return Err(format!("{SETTINGS}: unknown key `{key}`"));
    }
    let number = |key: &str, ok: fn(f64) -> bool, rule: &str| -> Result<Option<f64>, String> {
        let Some(item) = table.get(key) else {
            return Ok(None);
        };
        let value = item
            .as_float()
            .or_else(|| item.as_integer().map(|i| i as f64));
        match value.filter(|v| v.is_finite() && ok(*v)) {
            Some(v) => Ok(Some(v)),
            None => Err(format!("{SETTINGS}: `{key}` must be a number {rule}")),
        }
    };
    let required = |key: &str, ok: fn(f64) -> bool, rule: &str| {
        number(key, ok, rule)?.ok_or_else(|| format!("{SETTINGS}: missing key `{key}`"))
    };
    Ok(Settings {
        volume: required("volume", |v| v > 0.0 && v < 1.0, "above 0 and below 1")?,
        radius: required("radius", |v| v > 0.0, "above 0")?,
        young: required("young", |v| v > 0.0, "above 0")?,
        poisson: required("poisson", |v| v > -1.0 && v < 0.5, "above -1 and below 0.5")?,
        thickness: number("thickness", |v| v > 0.0, "above 0")?,
    })
}

/// The keyword of an input line in upper case with no spaces, for example `*ENDSTEP`.
fn keyword(line: &str) -> String {
    let word = line.split(',').next().unwrap_or_default();
    word.to_ascii_uppercase().replace(' ', "")
}

/// The input of the loop: `job.inp` with the materials and the sections of `density.inp`
/// before the `*STEP`, and the print request of the energy and the volume of each element before
/// the `*END STEP`. Without a `*NODE FILE`, the step also writes `U` to the `.frd` file, so that
/// the page has a frame for the density. Each `*NODE FILE` and `*EL FILE` gets `OUTPUT=3D`, if
/// it has no `OUTPUT`: `ccx` then writes 2D elements as the 3D elements that the page draws.
fn job_text(text: &str) -> Result<String, String> {
    let words: Vec<String> = text.lines().map(keyword).collect();
    let steps = words.iter().filter(|w| *w == "*STEP").count();
    if steps != 1 {
        return Err(format!(
            "job.inp of a topology case needs 1 *STEP, and it has {steps}"
        ));
    }
    if let Some(w) = words
        .iter()
        .find(|w| matches!(w.as_str(), "*MATERIAL" | "*SOLIDSECTION" | "*SHELLSECTION"))
    {
        return Err(format!(
            "job.inp of a topology case has no material and no section, because the loop writes \
             them: remove the {w}"
        ));
    }
    let field = if words.iter().any(|w| w == "*NODEFILE") {
        ""
    } else {
        "*NODE FILE, OUTPUT=3D\nU\n"
    };
    let mut out = String::new();
    for (line, word) in text.lines().zip(&words) {
        match word.as_str() {
            "*STEP" => out.push_str("*INCLUDE, INPUT=density.inp\n"),
            "*ENDSTEP" => {
                out.push_str(&format!("*EL PRINT, ELSET={SET}\nELSE, EVOL\n{field}"));
            }
            "*NODEFILE" | "*ELFILE" if !line.to_ascii_uppercase().contains("OUTPUT") => {
                out.push_str(line.trim_end().trim_end_matches(','));
                out.push_str(", OUTPUT=3D\n");
                continue;
            }
            _ => {}
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// The materials and the sections of the elements `ids` with the densities `x`, and the set of
/// all elements. `ccx` reads 20 characters of a number at most.
fn density_text(ids: &[u32], x: &[f64], s: &Settings) -> String {
    let emin = 1e-9 * s.young;
    let mut out = format!("*ELSET, ELSET={SET}\n");
    for row in ids.chunks(16) {
        let row: Vec<String> = row.iter().map(u32::to_string).collect();
        out.push_str(&format!("{},\n", row.join(", ")));
    }
    for (id, x) in ids.iter().zip(x) {
        let young = emin + x.powf(PENALTY) * (s.young - emin);
        out.push_str(&format!(
            "*ELSET, ELSET=FHD{id}\n{id}\n*MATERIAL, NAME=FHD{id}\n*ELASTIC\n\
             {young:.12e}, {:.12e}\n*SOLID SECTION, ELSET=FHD{id}, MATERIAL=FHD{id}\n",
            s.poisson
        ));
        if let Some(t) = s.thickness {
            out.push_str(&format!("{t:.12e}\n"));
        }
    }
    out
}

/// The value of each element of `ids` in the last block `title` of the `.dat` text, for example
/// `internal energy (element, energy) for set FHDESIGN`.
fn element_values(dat: &str, title: &str, ids: &[u32]) -> Result<Vec<f64>, String> {
    let mut block: Option<HashMap<u32, f64>> = None;
    let mut inside = false;
    for line in dat.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        match (words.first().map(|w| w.parse::<u32>()), words.get(1)) {
            (Some(Ok(id)), Some(value)) if inside && words.len() == 2 => {
                let value: f64 = value
                    .parse()
                    .map_err(|_| format!("{title}: bad row {line}"))?;
                block.get_or_insert_with(HashMap::new).insert(id, value);
            }
            _ => {
                inside = line.starts_with(title);
                if inside {
                    block = Some(HashMap::new());
                }
            }
        }
    }
    let block = block.ok_or_else(|| format!("job.dat has no {title}"))?;
    ids.iter()
        .map(|id| match block.get(id) {
            Some(v) if v.is_finite() => Ok(*v),
            _ => Err(format!("job.dat has no finite {title} for element {id}")),
        })
        .collect()
}

/// The filter: for each element, its neighbours and their weights max(0, radius - d).
fn neighbours(centres: &[[f64; 3]], radius: f64) -> Vec<Vec<(usize, f64)>> {
    let distance = |a: &[f64; 3], b: &[f64; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    centres
        .iter()
        .map(|c| {
            let near = centres.iter().enumerate();
            near.filter_map(|(i, d)| Some((i, radius - distance(c, d))).filter(|(_, w)| *w > 0.0))
                .collect()
        })
        .collect()
}

/// The densities after 1 iteration: the filtered sensitivities of the compliance and the
/// optimality criteria update. `energy` and `volumes` are the strain energy and the volume of
/// each element in the solve of the densities `x`.
fn update(
    x: &[f64],
    energy: &[f64],
    volumes: &[f64],
    filter: &[Vec<(usize, f64)>],
    s: &Settings,
) -> Vec<f64> {
    let emin = 1e-9 * s.young;
    // dc/dx = -p x^(p-1) (E0 - Emin) u k0 u, and the strain energy is E u k0 u / 2.
    let dc: Vec<f64> = x
        .iter()
        .zip(energy)
        .map(|(x, e)| {
            let young = emin + x.powf(PENALTY) * (s.young - emin);
            -PENALTY * x.powf(PENALTY - 1.0) * (s.young - emin) * 2.0 * e / young
        })
        .collect();
    let dc: Vec<f64> = filter
        .iter()
        .enumerate()
        .map(|(e, near)| {
            let sum: f64 = near.iter().map(|(i, w)| w * x[*i] * dc[*i]).sum();
            let weights: f64 = near.iter().map(|(_, w)| w).sum();
            sum / weights / x[e].max(1e-3)
        })
        .collect();
    let target = s.volume * volumes.iter().sum::<f64>();
    // At the upper bound each density falls, so the volume falls below the target.
    let most = dc
        .iter()
        .zip(volumes)
        .fold(0.0_f64, |m, (d, v)| m.max(-d / v));
    let (mut low, mut high) = (0.0, 1e9_f64.max(most / (1.0 - MOVE).powi(2)));
    let mut next = x.to_vec();
    while (high - low) / (low + high) > 1e-3 {
        let mid = 0.5 * (low + high);
        for (e, x) in x.iter().enumerate() {
            let b = x * (-dc[e] / volumes[e] / mid).max(0.0).sqrt();
            next[e] = b.min(x + MOVE).min(1.0).max(x - MOVE).max(0.0);
        }
        let filled: f64 = next.iter().zip(volumes).map(|(x, v)| x * v).sum();
        if filled > target {
            low = mid;
        } else {
            high = mid;
        }
    }
    next
}

/// One row of the history: the iteration, the compliance and the volume fraction of the solved
/// design, and the largest change of a density after the solve.
#[derive(Debug, Clone, Copy)]
pub struct Iteration {
    pub number: usize,
    pub compliance: f64,
    pub volume: f64,
    pub change: f64,
}

/// Optimize the case `case` in the folder `run_dir`, with `ccx` and its limits. The time limit
/// of `ccx` counts all the solves. The run clears the folder first, if a run made the folder
/// earlier. `on_iteration` gets each row of the history.
pub fn run_with(
    ccx: &Ccx,
    case: &Path,
    run_dir: &Path,
    cancel: &CancelToken,
    on_iteration: &mut dyn FnMut(&Iteration),
) -> Result<(), String> {
    let start = Instant::now();
    let case =
        fs::canonicalize(case).map_err(|e| format!("{}: case folder: {e}", case.display()))?;
    let toml = case.join(SETTINGS);
    let text = fs::read_to_string(&toml).map_err(|e| format!("{}: {e}", toml.display()))?;
    let s = settings(&text)?;
    let flat = flatten(&case.join("job.inp"))?;
    let job = job_text(&flat.text)?;
    let model = parse(&flat.text);
    let points: HashMap<u32, [f64; 3]> = model.nodes.iter().map(|n| (n.id, n.xyz)).collect();
    let mut centres = Vec::with_capacity(model.elements.len());
    for element in &model.elements {
        let mut c = [0.0; 3];
        for id in &element.nodes {
            let p = points
                .get(id)
                .ok_or_else(|| format!("element {} has the unknown node {id}", element.id))?;
            (0..3).for_each(|k| c[k] += p[k] / element.nodes.len() as f64);
        }
        centres.push(c);
    }
    let ids: Vec<u32> = model.elements.iter().map(|e| e.id).collect();
    if ids.is_empty() {
        return Err("job.inp of the topology case has no element".into());
    }
    let filter = neighbours(&centres, s.radius);

    let run_dir = prepare_run_dir(run_dir, &case)?;
    let work = run_dir.join(WORK);
    let folder = |e: std::io::Error| format!("run folder: {e}");
    fs::create_dir_all(&work).map_err(folder)?;
    fs::write(work.join("job.inp"), job).map_err(folder)?;
    let solve = work.join("solve");
    let mut history = String::from("# iteration compliance volume change\n");
    let mut x = vec![s.volume; ids.len()];
    let mut volumes = Vec::new();
    let mut solved = x.clone();
    for number in 1..=ITERATIONS {
        if cancel.is_cancelled() {
            return Err("run cancelled".into());
        }
        fs::write(work.join("density.inp"), density_text(&ids, &x, &s)).map_err(folder)?;
        let mut each = ccx.clone();
        each.timeout = ccx.timeout.map(|t| t.saturating_sub(start.elapsed()));
        each.run_with(&work.join("job.inp"), &solve, cancel, &mut |_| {})
            .map_err(|e| match ccx.timeout {
                Some(t) if start.elapsed() >= t => format!(
                    "the run stopped at the time limit of {} s after {} iterations. Use a \
                     coarser mesh",
                    t.as_secs_f64(),
                    number - 1
                ),
                _ => e,
            })?;
        let file = solve.join("ccx/job.dat");
        let dat = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let energy = element_values(&dat, "internal energy (element, energy)", &ids)?;
        if volumes.is_empty() {
            volumes = element_values(&dat, "volume (element, volume)", &ids)?;
            if let Some(i) = volumes.iter().position(|v| *v <= 0.0) {
                return Err(format!("element {} has no volume", ids[i]));
            }
        }
        let next = update(&x, &energy, &volumes, &filter, &s);
        let filled: f64 = x.iter().zip(&volumes).map(|(x, v)| x * v).sum();
        let row = Iteration {
            number,
            compliance: 2.0 * energy.iter().sum::<f64>(),
            volume: filled / volumes.iter().sum::<f64>(),
            change: next
                .iter()
                .zip(&x)
                .fold(0.0_f64, |m, (a, b)| m.max((a - b).abs())),
        };
        history.push_str(&format!(
            "{} {:.10e} {:.6} {:.6}\n",
            row.number, row.compliance, row.volume, row.change
        ));
        fs::write(work.join(HISTORY), &history).map_err(folder)?;
        on_iteration(&row);
        solved = std::mem::replace(&mut x, next);
        if row.change <= CHANGE {
            break;
        }
    }
    let file = solve.join("ccx/job.frd");
    let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let results = frd::read(&String::from_utf8_lossy(&bytes))?;
    let at: HashMap<u32, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let density: Vec<f64> = (results.elements.iter())
        .map(|id| at.get(id).map_or(0.0, |i| solved[*i]))
        .collect();
    let units = fh_core::inp::units(&flat.text);
    let units = match units.as_str() {
        "SI-mm" | "SI-m" => units.as_str(),
        _ => "unspecified",
    };
    let frames = results.frames.iter().map(|f| {
        let mut grid = results.grid(f);
        grid.cell_data
            .push(Field::f64("density", 1, density.clone()));
        (f.time, grid)
    });
    write_frames(&run_dir, units, false, frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_of(volume: f64) -> Settings {
        Settings {
            volume,
            radius: 1.5,
            young: 1.0,
            poisson: 0.3,
            thickness: None,
        }
    }

    #[test]
    fn the_settings_need_the_4_keys_in_their_ranges() {
        let text = "volume = 0.5\nradius = 2.4\nyoung = 1\npoisson = 0.3\n";
        let s = settings(text).unwrap();
        assert_eq!(
            s,
            Settings {
                radius: 2.4,
                volume: 0.5,
                ..settings_of(0.5)
            }
        );
        assert_eq!(
            settings(&format!("{text}thickness = 2\n"))
                .unwrap()
                .thickness,
            Some(2.0)
        );
        let error = |text: &str| settings(text).unwrap_err();
        assert!(error(&text.replace("0.5", "1"))
            .contains("`volume` must be a number above 0 and below 1"));
        assert!(error(&text.replace("radius = 2.4\n", "")).contains("missing key `radius`"));
        assert!(error(&format!("{text}penalty = 3\n")).contains("unknown key `penalty`"));
        assert!(error(&text.replace("1\n", "\"steel\"\n")).contains("`young` must be a number"));
    }

    #[test]
    fn the_input_gets_the_densities_before_the_step_and_the_print_request_in_it() {
        let job = "*NODE\n1, 0, 0\n*Step\n*STATIC\n*End Step\n";
        let text = job_text(job).unwrap();
        assert_eq!(
            text,
            "*NODE\n1, 0, 0\n*INCLUDE, INPUT=density.inp\n*Step\n*STATIC\n\
             *EL PRINT, ELSET=FHDESIGN\nELSE, EVOL\n*NODE FILE, OUTPUT=3D\nU\n*End Step\n"
        );
        let fields = job.replace(
            "*End Step",
            "*Node File,\nU, RF\n*EL FILE, OUTPUT=2D\nS\n*End Step",
        );
        let text = job_text(&fields).unwrap();
        assert!(
            text.ends_with(
                "*Node File, OUTPUT=3D\nU, RF\n*EL FILE, OUTPUT=2D\nS\n\
                 *EL PRINT, ELSET=FHDESIGN\nELSE, EVOL\n*End Step\n"
            ),
            "{text}"
        );
        let two = format!("{job}{job}");
        assert!(job_text(&two)
            .unwrap_err()
            .contains("needs 1 *STEP, and it has 2"));
        let section = job.replace("*Step", "*SOLID SECTION, ELSET=ALL, MATERIAL=STEEL\n*Step");
        assert!(job_text(&section)
            .unwrap_err()
            .contains("remove the *SOLIDSECTION"));
        // A comment is not a keyword.
        assert!(job_text(&job.replace("*NODE", "**STEP\n*NODE")).is_ok());
    }

    #[test]
    fn each_element_gets_its_material_with_the_penalized_modulus() {
        let mut s = settings_of(0.5);
        s.thickness = Some(2.0);
        let text = density_text(&[3, 8], &[1.0, 0.5], &s);
        assert!(
            text.starts_with("*ELSET, ELSET=FHDESIGN\n3, 8,\n"),
            "{text}"
        );
        assert!(
            text.contains("*MATERIAL, NAME=FHD3\n*ELASTIC\n1.000000000000e0, 3.000000000000e-1\n")
        );
        // 1e-9 + 0.5^3 (1 - 1e-9).
        assert!(text.contains("*ELASTIC\n1.250000008750e-1, "), "{text}");
        assert!(text.contains("*SOLID SECTION, ELSET=FHD8, MATERIAL=FHD8\n2.000000000000e0\n"));
        assert!(text
            .lines()
            .all(|l| l.split(',').all(|w| w.trim().len() <= 20)));
    }

    #[test]
    fn the_values_come_from_the_last_block_of_the_title() {
        let dat = "\n internal energy (element, energy) for set FHDESIGN and time  0.1E+01\n\n\
            \x20        1  9.0E+00\n         2  5.0E+00\n\n\
            \x20volume (element, volume) for set FHDESIGN and time  0.1E+01\n\n\
            \x20        1  1.0E+00\n         2  2.0E+00\n\n\
            \x20internal energy (element, energy) for set FHDESIGN and time  0.2E+01\n\n\
            \x20        2  4.0E+00\n         1  8.0E+00\n";
        let energy = element_values(dat, "internal energy (element, energy)", &[1, 2]);
        assert_eq!(energy, Ok(vec![8.0, 4.0]));
        let volume = element_values(dat, "volume (element, volume)", &[2, 1]);
        assert_eq!(volume, Ok(vec![2.0, 1.0]));
        let missing = element_values(dat, "volume (element, volume)", &[3]).unwrap_err();
        assert!(missing.contains("for element 3"), "{missing}");
    }

    /// The update keeps the volume, moves each density by 0.2 at most, and adds material where
    /// the strain energy is high.
    #[test]
    fn the_update_keeps_the_volume_and_the_move_limit() {
        let centres: Vec<[f64; 3]> = (0..6).map(|i| [f64::from(i), 0.0, 0.0]).collect();
        let filter = neighbours(&centres, 1.5);
        assert_eq!(filter[0], [(0, 1.5), (1, 0.5)]);
        let x = vec![0.5; 6];
        let energy = [6.0, 5.0, 1.0, 1.0, 0.5, 0.1];
        let volumes = [1.0, 1.0, 1.0, 1.0, 2.0, 2.0];
        let next = update(&x, &energy, &volumes, &filter, &settings_of(0.5));
        let filled: f64 = next.iter().zip(&volumes).map(|(x, v)| x * v).sum();
        assert!((filled - 4.0).abs() < 4e-3, "{filled} {next:?}");
        assert!(
            next.iter().all(|v| (v - 0.5).abs() <= MOVE + 1e-12),
            "{next:?}"
        );
        assert!(next[0] > 0.5 && next[5] < 0.5, "{next:?}");
    }
}
