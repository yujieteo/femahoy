//! GetDP as the solver of femahoy for electromagnetics and acoustics, in the process of `fh`.
//!
//! A GetDP case is a folder with `case.pro`, and `case.geo` or `case.msh`. [`GetDp::run`] copies
//! the files of the folder to `<run-dir>/getdp` and starts there:
//!
//! 1. gmsh (see [`crate::msh::find_gmsh`]), when the case has `case.geo`:
//!    `gmsh case.geo -3 -o case.msh`;
//! 2. the user's `getdp` (`$FH_GETDP`, else `getdp` on PATH):
//!    `getdp case.pro -msh case.msh -solve #1 -pos #1`. This runs the first `Resolution` and the
//!    first `PostOperation` of `case.pro`.
//!
//! Each program writes its output to `<run-dir>/getdp/log.<program>`. The PostOperation writes
//! its text results (for example `Format Table`) to the files that it names. Then the run writes
//! the views of the `.pos` files (the default format of GetDP) to `step-N.vtu` and
//! `results.pvd`: step N of each view as a cell field, the mean of the node values of each
//! element. A complex resolution has 2 steps: the real part and the imaginary part. Only a vector
//! view `U` is a point field, because the viewer takes a point field `U` as a displacement.
//!
//! GetDP and gmsh are GPL, so femahoy only starts them as separate programs. A run has the
//! limits of a run of `ccx` (see [`crate::ccx`]). The time limit counts both programs.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use fh_core::vtu::{Field, Grid};

use crate::ccx::{
    default_memory_limit, log_tail, prepare_run_dir, supervise, write_frames, CancelToken, Program,
};

/// The folder of the GetDP files in a run folder.
pub const WORK: &str = "getdp";

/// True when `dir` is a GetDP case: it has `case.pro`.
pub fn is_case(dir: &Path) -> bool {
    dir.join("case.pro").is_file()
}

/// Copy the files of the case `case` (not its folders) to `dir`. With `read_only`, each copied
/// file is read-only, else the owner can write it.
pub(crate) fn copy_case(case: &Path, dir: &Path, read_only: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let copy = || -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        for entry in fs::read_dir(case)? {
            let src = entry?.path();
            if src.is_file() {
                let dst = dir.join(src.file_name().expect("a file has a name"));
                fs::copy(&src, &dst)?;
                let mut perms = fs::metadata(&dst)?.permissions();
                if read_only {
                    perms.set_readonly(true);
                } else {
                    perms.set_mode(perms.mode() | 0o200);
                }
                fs::set_permissions(&dst, perms)?;
            }
        }
        Ok(())
    };
    copy().map_err(|e| format!("case copy: {}: {e}", case.display()))
}

/// Why `program` failed, from its exit status and its output `log`. gmsh and GetDP start each
/// error line with `Error`.
pub(crate) fn failure(program: &str, status: ExitStatus, log: &str, log_file: &Path) -> String {
    let see = format!("The full output is in {}", log_file.display());
    let errors: Vec<&str> = log
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("Error"))
        .collect();
    if errors.is_empty() {
        return format!(
            "{program} failed ({status}). {see}. Its last lines: {}",
            log_tail(log)
        );
    }
    format!(
        "{program} failed ({status}): {}. Correct the case as the message tells. {see}.",
        errors.join(" | ")
    )
}

/// One view of a `.pos` file. `cells` holds the VTK type and the corner points of each element.
/// `steps` holds for each step the values at the corners of each element, in the order of the
/// file, `components` numbers at each corner.
#[derive(Debug, Default)]
struct View {
    name: String,
    components: usize,
    cells: Vec<(u8, Vec<[f64; 3]>)>,
    steps: Vec<Vec<f64>>,
    times: Vec<f64>,
}

/// The node count and the VTK type of the element letter of the parsed format.
fn shape(letter: &str) -> Option<(usize, u8)> {
    Some(match letter {
        "P" => (1, 1),
        "L" => (2, 3),
        "T" => (3, 5),
        "Q" => (4, 9),
        "S" => (4, 10),
        "H" => (8, 12),
        "I" => (6, 13),
        "Y" => (5, 14),
        _ => return None,
    })
}

fn numbers(text: &str) -> Result<Vec<f64>, String> {
    text.split(',')
        .map(|w| {
            let w = w.trim();
            w.parse()
                .map_err(|_| format!("{w:?} is where a number must be"))
        })
        .collect()
}

/// Read the views of a `.pos` file in the parsed format of gmsh, as GetDP writes it: one element
/// on each line, for example `SS(x1,y1,z1,...){v1,...};`, and `TIME{t1,...};`.
fn read_pos(text: &str) -> Result<Vec<View>, String> {
    let mut views: Vec<View> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let at = |e: String| format!("line {}: {e}", i + 1);
        let line = line.trim();
        if line.is_empty() || line == "};" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("View ") {
            let name = rest.trim_end_matches('{').trim().trim_matches('"');
            views.push(View {
                name: name.to_owned(),
                ..View::default()
            });
            continue;
        }
        let view = views
            .last_mut()
            .ok_or_else(|| at("an element comes before a View".into()))?;
        let bad = || at(format!("{line:?} is not an element of the parsed format"));
        let (head, body) = line.split_once('{').ok_or_else(bad)?;
        let values = numbers(body.strip_suffix("};").ok_or_else(bad)?).map_err(at)?;
        if head == "TIME" {
            view.times = values;
            continue;
        }
        let (code, coords) = head.split_once('(').ok_or_else(bad)?;
        let coords = numbers(coords.strip_suffix(')').ok_or_else(bad)?).map_err(at)?;
        let components = match code.get(..1) {
            Some("S") => 1,
            Some("V") => 3,
            Some("T") => 9,
            _ => 0,
        };
        let Some((nodes, vtk)) = code.get(1..).and_then(shape).filter(|_| components > 0) else {
            return Err(at(format!(
                "the element code {code} is not a first-order element of the parsed format"
            )));
        };
        let size = nodes * components;
        if coords.len() != 3 * nodes || values.is_empty() || values.len() % size != 0 {
            return Err(at(format!(
                "{code} has {} numbers",
                coords.len() + values.len()
            )));
        }
        if view.cells.is_empty() {
            view.components = components;
            view.steps = vec![Vec::new(); values.len() / size];
        }
        if components != view.components || values.len() != size * view.steps.len() {
            return Err(at(format!(
                "{code} does not match the elements of view {}",
                view.name
            )));
        }
        for (step, chunk) in view.steps.iter_mut().zip(values.chunks_exact(size)) {
            step.extend_from_slice(chunk);
        }
        let points = coords.chunks_exact(3).map(|p| [p[0], p[1], p[2]]);
        view.cells.push((vtk, points.collect()));
    }
    Ok(views)
}

/// The frames of the views: frame N holds step N of each view that has it. All the views must
/// have the same elements, because a VTU file has 1 mesh. A view is a cell field, the mean of the
/// corner values of each element. A view `U` with 3 components is the point field `U`. The points
/// of the elements merge when they have the same coordinates and the same `U` in each step.
fn frames(views: &[View]) -> Result<Vec<(f64, Grid)>, String> {
    let Some(first) = views.first() else {
        return Ok(Vec::new());
    };
    if let Some(other) = views.iter().find(|v| v.cells != first.cells) {
        return Err(format!(
            "view {} has other elements than view {}. A VTU file has 1 mesh, so print each \
             view OnElementsOf the same region",
            other.name, first.name
        ));
    }
    let is_u = |v: &View| v.name == "U" && v.components == 3;
    let u = views.iter().find(|v| is_u(v));
    let mut grid = Grid::default();
    // The corner of the elements that gives each point its value of `U`.
    let mut corners = Vec::new();
    let mut ids: HashMap<Vec<u64>, u32> = HashMap::new();
    for (vtk, points) in &first.cells {
        let mut cell = Vec::with_capacity(points.len());
        for p in points {
            let corner = corners.len();
            let mut key = p.map(f64::to_bits).to_vec();
            for step in u.map_or(&[][..], |u| &u.steps) {
                key.extend(step[3 * corner..3 * corner + 3].iter().map(|x| x.to_bits()));
            }
            let id = *ids.entry(key).or_insert_with(|| {
                grid.points.push(*p);
                grid.points.len() as u32 - 1
            });
            cell.push(id);
            corners.push(id);
        }
        grid.cells.push(cell);
        grid.cell_types.push(*vtk);
    }
    let steps = views.iter().map(|v| v.steps.len()).max().unwrap_or(0);
    Ok((0..steps)
        .map(|k| {
            let mut frame = grid.clone();
            for v in views.iter().filter(|v| k < v.steps.len()) {
                let values = &v.steps[k];
                if is_u(v) {
                    let mut point = vec![0.0; 3 * grid.points.len()];
                    for (corner, id) in corners.iter().enumerate() {
                        let id = *id as usize;
                        point[3 * id..3 * id + 3]
                            .copy_from_slice(&values[3 * corner..3 * corner + 3]);
                    }
                    frame.point_data.push(Field::f64("U", 3, point));
                    continue;
                }
                let mut means = Vec::with_capacity(first.cells.len() * v.components);
                let mut at = 0;
                for (_, points) in &first.cells {
                    let size = points.len() * v.components;
                    for c in 0..v.components {
                        let sum: f64 = values[at + c..at + size].iter().step_by(v.components).sum();
                        means.push(sum / points.len() as f64);
                    }
                    at += size;
                }
                frame
                    .cell_data
                    .push(Field::f64(&v.name, v.components, means));
            }
            (first.times.get(k).copied().unwrap_or(k as f64), frame)
        })
        .collect())
}

/// The `getdp` program with the limits of its runs.
#[derive(Debug, Clone)]
pub struct GetDp {
    /// `$FH_GETDP`, else `getdp` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Limit for the process group of a program in bytes, as for `ccx`. `u64::MAX` means no
    /// limit.
    pub memory_limit: u64,
}

impl Default for GetDp {
    fn default() -> Self {
        Self::new()
    }
}

impl GetDp {
    /// `getdp` from `$FH_GETDP`, else `getdp` on PATH. It has no time limit. The memory limit
    /// is 85 % of the machine RAM.
    pub fn new() -> Self {
        GetDp {
            exe: std::env::var_os("FH_GETDP").map_or_else(|| PathBuf::from("getdp"), PathBuf::from),
            timeout: None,
            memory_limit: default_memory_limit(),
        }
    }

    /// Solve the case `case` in the folder `run_dir`. The run clears the folder first, if a run
    /// made the folder earlier.
    pub fn run(&self, case: &Path, run_dir: &Path) -> Result<(), String> {
        self.run_with(case, run_dir, &CancelToken::new(), &mut |_| {})
    }

    /// Solve, and stop when `cancel` is set. `on_program` gets the name of each program that
    /// the run starts.
    pub fn run_with(
        &self,
        case: &Path,
        run_dir: &Path,
        cancel: &CancelToken,
        on_program: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        let start = Instant::now();
        let case =
            fs::canonicalize(case).map_err(|e| format!("{}: case folder: {e}", case.display()))?;
        if !is_case(&case) {
            return Err(format!(
                "{} is not a GetDP case: it has no case.pro",
                case.display()
            ));
        }
        let run_dir = prepare_run_dir(run_dir, &case)?;
        let work = run_dir.join(WORK);
        copy_case(&case, &work, false)?;
        let mut programs: Vec<(Program, PathBuf, Vec<&str>)> = Vec::new();
        if work.join("case.geo").is_file() {
            let gmsh = Program {
                name: "gmsh",
                install: "Install gmsh (README.md) and put gmsh on PATH, or set FH_GMSH",
            };
            let args = vec!["case.geo", "-3", "-o", "case.msh"];
            programs.push((gmsh, crate::msh::find_gmsh()?, args));
        }
        let getdp = Program {
            name: "getdp",
            install: "Install GetDP (README.md) and put getdp on PATH, or set FH_GETDP",
        };
        let args = vec!["case.pro", "-msh", "case.msh", "-solve", "#1", "-pos", "#1"];
        programs.push((getdp, self.exe.clone(), args));
        for (program, exe, args) in &programs {
            on_program(program.name);
            let log_file = work.join(format!("log.{}", program.name));
            let folder = |e: std::io::Error| format!("run folder: {e}");
            let log = fs::File::create(&log_file).map_err(folder)?;
            let mut cmd = Command::new(exe);
            cmd.args(args)
                .current_dir(&work)
                // gmsh reads `~/.gmsh-options`. The run folder has none.
                .env("HOME", &work)
                .stdin(Stdio::null())
                .stdout(log.try_clone().map_err(folder)?)
                .stderr(log);
            let limits = (start, self.timeout, self.memory_limit);
            let status = supervise(program, &mut cmd, limits, cancel, &mut || {})?;
            if !status.success() {
                let log = fs::read_to_string(&log_file).unwrap_or_default();
                return Err(failure(program.name, status, &log, &log_file));
            }
        }
        write_views(&work, &run_dir, "unspecified")
    }
}

/// Write the views of the `.pos` files in `work` to the VTU files and `results.pvd` of
/// `run_dir`, with the unit system `units`.
pub(crate) fn write_views(work: &Path, run_dir: &Path, units: &str) -> Result<(), String> {
    let mut files: Vec<PathBuf> = fs::read_dir(work)
        .map_err(|e| format!("{}: {e}", work.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pos"))
        .collect();
    files.sort();
    let mut views = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        views.extend(read_pos(&text).map_err(|e| format!("{}: {e}", file.display()))?);
    }
    write_frames(run_dir, units, frames(&views)?.into_iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2 tetrahedra with a common face, a complex vector view and a real scalar view.
    const POS: &str = "View \"b\" {\n\
        VS(0,0,0,1,0,0,0,1,0,0,0,1){1,0,0,1,0,0,1,0,0,1,0,4,0,0,8,0,0,8,0,0,8,0,0,8};\n\
        VS(1,0,0,0,1,0,0,0,1,1,1,1){2,0,0,2,0,0,2,0,0,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0};\n\
        TIME{0,1};\n};\n\
        View \"loss\" {\n\
        SS(0,0,0,1,0,0,0,1,0,0,0,1){1,2,3,4};\n\
        SS(1,0,0,0,1,0,0,0,1,1,1,1){0,0,0,0};\n};\n";

    #[test]
    fn the_views_give_1_frame_for_each_step_with_the_cell_means() {
        let frames = frames(&read_pos(POS).unwrap()).unwrap();
        assert_eq!(frames.len(), 2);
        let (time, grid) = &frames[0];
        assert_eq!(*time, 0.0);
        // The 3 points of the common face merge.
        assert_eq!(grid.points.len(), 5);
        assert_eq!(grid.cells, vec![vec![0, 1, 2, 3], vec![1, 2, 3, 4]]);
        assert_eq!(grid.cell_types, vec![10, 10]);
        assert!(grid.point_data.is_empty());
        let b = grid.cell_field("b").unwrap();
        assert_eq!(
            (b.components, b.as_f64()),
            (3, Some(&[1.0, 0.0, 1.0, 2.0, 0.0, 0.0][..]))
        );
        assert_eq!(
            grid.cell_field("loss").unwrap().as_f64(),
            Some(&[2.5, 0.0][..])
        );
        // The imaginary part. The real view has no step 2.
        let (time, grid) = &frames[1];
        assert_eq!(*time, 1.0);
        let b = grid.cell_field("b").unwrap();
        assert_eq!(b.as_f64(), Some(&[0.0, 0.0, 8.0, 0.0, 0.0, 0.0][..]));
        assert!(grid.cell_field("loss").is_none());
    }

    /// 3 bars in a row. The first 2 have a pin, so their common point has the same `U`. The
    /// third bar starts at the end of the second, but moves in z.
    #[test]
    fn the_view_u_is_a_point_field_and_only_points_that_move_together_merge() {
        let pos = "View \"U\" {\n\
            VL(0,0,0,1,0,0){0,0,0,0,0,0,0,0,0,0,1,0};\n\
            VL(1,0,0,2,0,0){0,0,0,0,0,0,0,1,0,0,2,0};\n\
            VL(2,0,0,3,0,0){0,0,0,0,0,0,0,0,1,0,0,1};\n\
            TIME{0,0.5};\n};\n";
        let frames = frames(&read_pos(pos).unwrap()).unwrap();
        let (time, grid) = &frames[1];
        assert_eq!(*time, 0.5);
        assert_eq!(grid.cells, vec![vec![0, 1], vec![1, 2], vec![3, 4]]);
        assert!(grid.cell_data.is_empty());
        let u = grid.point_field("U").unwrap();
        let want = [
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
        ];
        assert_eq!((u.components, u.as_f64()), (3, Some(&want[..])));
    }

    #[test]
    fn a_bad_pos_file_is_an_error() {
        let short = POS.replacen("{1,2,3,4}", "{1,2,3}", 1);
        assert!(read_pos(&short).unwrap_err().contains("line 7"));
        let word = POS.replacen("{1,2,3,4}", "{1,2,x,4}", 1);
        assert!(read_pos(&word).unwrap_err().contains("\"x\""));
        let second_order = POS.replacen("SS(", "SS2(", 1);
        assert!(read_pos(&second_order).unwrap_err().contains("SS2"));
        let one = POS.replacen("SS(1,0,0,0,1,0,0,0,1,1,1,1){0,0,0,0};\n", "", 1);
        let err = frames(&read_pos(&one).unwrap()).unwrap_err();
        assert!(
            err.contains("view loss has other elements than view b"),
            "{err}"
        );
    }

    #[test]
    fn a_failure_shows_the_error_lines() {
        let status = Command::new("/bin/sh")
            .args(["-c", "exit 1"])
            .status()
            .unwrap();
        let log = "Info    : Loading problem definition 'case.pro'\n\
            Error   : Unknown Resolution (Nope)\n";
        let text = failure("getdp", status, log, Path::new("log.getdp"));
        assert!(
            text.contains("getdp failed (exit status: 1): Error   : Unknown Resolution (Nope)."),
            "{text}"
        );
        assert!(text.contains("log.getdp"), "{text}");
        let plain = failure("gmsh", status, "Info: done\n", Path::new("log.gmsh"));
        assert!(plain.contains("Its last lines: Info: done"), "{plain}");
    }
}
