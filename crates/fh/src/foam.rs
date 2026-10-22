//! OpenFOAM as the CFD solver of femahoy, in the process of `fh`.
//!
//! An OpenFOAM case is a folder with `system/controlDict`, `constant` and `0`. [`Foam::run`]
//! copies these 3 folders to `<run-dir>/foam` and starts the user's `openfoam` (`$FH_OPENFOAM`,
//! else `openfoam` on PATH) there for each program in turn:
//!
//! 1. `blockMesh`, when the case has `system/blockMeshDict` and no `constant/polyMesh`;
//! 2. `setFields`, when the case has `system/setFieldsDict`, for example to put the water in an
//!    `interFoam` case;
//! 3. the application that `system/controlDict` names, for example `simpleFoam`;
//! 4. `foamToVTK` for the last time.
//!
//! Each program writes its output to `<run-dir>/foam/log.<program>`. The function objects of
//! the case write their text results to `<run-dir>/foam/postProcessing`. Then the run writes the
//! cell fields of the last time (for example `U`, `p` and `nut`) to `step-1.vtu` and
//! `results.pvd`. The VTU file has no point fields, because the viewer takes a point field `U`
//! as a displacement.
//!
//! OpenFOAM is GPL, so femahoy only starts it as a separate program. A run has the limits of a
//! run of `ccx` (see [`crate::ccx`]). The time limit counts all the programs of a run.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use fh_core::vtu::{Field, Grid};

use crate::ccx::{
    default_memory_limit, log_tail, prepare_run_dir, supervise, write_frames, CancelToken, Program,
};

/// The folder of the OpenFOAM files in a run folder.
pub const WORK: &str = "foam";
/// The folders of a case that a run copies.
const CASE_DIRS: [&str; 3] = ["system", "constant", "0"];
/// The last program of a run: the cell fields of the last time as 1 legacy VTK file.
pub(crate) const TO_VTK: [&str; 6] = [
    "foamToVTK",
    "-latestTime",
    "-legacy",
    "-ascii",
    "-no-boundary",
    "-no-point-data",
];

/// True when `dir` is an OpenFOAM case: it has `system/controlDict`.
pub fn is_case(dir: &Path) -> bool {
    dir.join("system/controlDict").is_file()
}

/// Copy the folders `system`, `constant` and `0` of the case `case` to `dir`. With `read_only`,
/// each copied file is read-only.
pub(crate) fn copy_case(case: &Path, dir: &Path, read_only: bool) -> Result<(), String> {
    fn tree(from: &Path, to: &Path, read_only: bool) -> std::io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let (src, dst) = (entry.path(), to.join(entry.file_name()));
            if src.is_dir() {
                tree(&src, &dst, read_only)?;
            } else {
                fs::copy(&src, &dst)?;
                let mut perms = fs::metadata(&dst)?.permissions();
                perms.set_readonly(read_only);
                fs::set_permissions(&dst, perms)?;
            }
        }
        Ok(())
    }
    for name in CASE_DIRS {
        let from = case.join(name);
        if from.is_dir() {
            tree(&from, &dir.join(name), read_only)
                .map_err(|e| format!("case copy: {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// The application of the case: the word after `application` in `system/controlDict`.
pub(crate) fn application(case: &Path) -> Result<String, String> {
    let file = case.join("system/controlDict");
    let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    text.lines()
        .find_map(|l| l.trim().strip_prefix("application"))
        .and_then(|rest| rest.trim().strip_suffix(';'))
        .map(str::trim)
        .filter(|app| !app.is_empty() && app.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "{}: no `application <name>;` line, for example `application simpleFoam;`",
                file.display()
            )
        })
}

/// Why a program of OpenFOAM failed and what to do, from its exit status and its output `log`.
pub(crate) fn failure(program: &str, status: ExitStatus, log: &str, log_file: &Path) -> String {
    let see = format!(
        "The full output is in {}. Its last lines: {}",
        log_file.display(),
        log_tail(log)
    );
    // The first fatal error: its lines up to the line that tells where in OpenFOAM it is.
    if let Some(at) = log.find("FOAM FATAL") {
        let words: Vec<&str> = log[at..]
            .lines()
            .take_while(|l| !l.trim_start().starts_with("From ") && !l.contains("FOAM exiting"))
            .flat_map(str::split_whitespace)
            .collect();
        return format!(
            "{program} failed ({status}): {}. Correct the case as the message tells. {see}",
            words.join(" ").trim_end_matches('.')
        );
    }
    // The OpenFOAM adapter of preCICE (see crate::precice).
    if let Some(e) = crate::precice::error_line(log) {
        return format!(
            "{program} failed ({status}): {e}. Correct the case as the message tells. {see}"
        );
    }
    if log.contains("sigFpe") {
        return format!(
            "{program} failed ({status}) with a floating point exception: usually the \
             solution diverged. Check the mesh with checkMesh, or use smaller relaxation \
             factors. {see}"
        );
    }
    format!("{program} failed ({status}). {see}")
}

/// The words of a legacy VTK file, read one at a time.
struct Words<'a>(Box<dyn Iterator<Item = &'a str> + 'a>);

impl<'a> Words<'a> {
    fn word(&mut self) -> Result<&'a str, String> {
        self.0
            .next()
            .ok_or_else(|| "the VTK file ends too early".to_owned())
    }

    fn number<T: std::str::FromStr>(&mut self) -> Result<T, String> {
        let word = self.word()?;
        word.parse()
            .map_err(|_| format!("the VTK file has {word:?} where a number must be"))
    }

    fn numbers<T: std::str::FromStr>(&mut self, n: usize) -> Result<Vec<T>, String> {
        (0..n).map(|_| self.number()).collect()
    }
}

/// Read the legacy ASCII VTK file of `foamToVTK -legacy -ascii -no-point-data`: the time, the
/// points, the cells and the cell fields.
fn read_legacy(text: &str) -> Result<(f64, Grid), String> {
    // The first 2 lines are the version and the title.
    let mut w = Words(Box::new(
        text.lines().skip(2).flat_map(str::split_whitespace),
    ));
    let (mut grid, mut time, mut cells) = (Grid::default(), 0.0, false);
    while let Some(key) = w.0.next() {
        match key {
            "ASCII" => {}
            "DATASET" => match w.word()? {
                "UNSTRUCTURED_GRID" => {}
                other => return Err(format!("the VTK file has a {other}, not a grid")),
            },
            "POINTS" => {
                let n: usize = w.number()?;
                w.word()?;
                let flat: Vec<f64> = w.numbers(3 * n)?;
                grid.points = flat.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
            }
            "CELLS" => {
                let n: usize = w.number()?;
                w.word()?;
                for _ in 0..n {
                    let size = w.number()?;
                    grid.cells.push(w.numbers(size)?);
                }
            }
            "CELL_TYPES" => {
                let n = w.number()?;
                grid.cell_types = w.numbers(n)?;
            }
            "CELL_DATA" => {
                w.word()?;
                cells = true;
            }
            "FIELD" => {
                w.word()?;
                let arrays: usize = w.number()?;
                for _ in 0..arrays {
                    let name = w.word()?;
                    let (components, count): (usize, usize) = (w.number()?, w.number()?);
                    w.word()?;
                    let values: Vec<f64> = w.numbers(components * count)?;
                    if name == "TimeValue" {
                        time = values.first().copied().unwrap_or(0.0);
                    } else if cells {
                        if count != grid.cells.len() {
                            return Err(format!("cell field {name} has {count} values"));
                        }
                        grid.cell_data.push(Field::f64(name, components, values));
                    }
                }
            }
            other => return Err(format!("the VTK file has an unknown keyword {other}")),
        }
    }
    let n = grid.points.len();
    if grid.cell_types.len() != grid.cells.len()
        || grid.cells.iter().flatten().any(|p| *p as usize >= n)
    {
        return Err("the cells of the VTK file do not match its points".into());
    }
    Ok((time, grid))
}

/// The `openfoam` program with the limits of its runs.
#[derive(Debug, Clone)]
pub struct Foam {
    /// `$FH_OPENFOAM`, else `openfoam` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Limit for the process group of a program in bytes, as for `ccx`. `u64::MAX` means no
    /// limit.
    pub memory_limit: u64,
}

impl Default for Foam {
    fn default() -> Self {
        Self::new()
    }
}

impl Foam {
    /// `openfoam` from `$FH_OPENFOAM`, else `openfoam` on PATH. It has no time limit. The
    /// memory limit is 85 % of the machine RAM.
    pub fn new() -> Self {
        Foam {
            exe: std::env::var_os("FH_OPENFOAM")
                .map_or_else(|| PathBuf::from("openfoam"), PathBuf::from),
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
                "{} is not an OpenFOAM case: it has no system/controlDict",
                case.display()
            ));
        }
        let app = application(&case)?;
        let run_dir = prepare_run_dir(run_dir, &case)?;
        let work = run_dir.join(WORK);
        copy_case(&case, &work, false)?;
        let mut programs: Vec<Vec<&str>> = Vec::new();
        if work.join("system/blockMeshDict").is_file() && !work.join("constant/polyMesh").exists() {
            programs.push(vec!["blockMesh"]);
        }
        if work.join("system/setFieldsDict").is_file() {
            programs.push(vec!["setFields"]);
        }
        programs.push(vec![&app]);
        programs.push(TO_VTK.to_vec());
        for args in &programs {
            on_program(args[0]);
            let limits = (start, self.timeout, self.memory_limit);
            self.program(args, &work, limits, cancel, &mut || {})?;
        }
        write_result(&work, &run_dir)
    }

    /// Run the OpenFOAM program `args` on the case in `work`, with its output in
    /// `work/log.<program>`.
    pub(crate) fn program(
        &self,
        args: &[&str],
        work: &Path,
        limits: (Instant, Option<Duration>, u64),
        cancel: &CancelToken,
        poll: &mut dyn FnMut(),
    ) -> Result<(), String> {
        let program = Program {
            name: "openfoam",
            install: "Install OpenFOAM (README.md) and put openfoam on PATH, or set FH_OPENFOAM",
        };
        let log_file = work.join(format!("log.{}", args[0]));
        let folder = |e: std::io::Error| format!("run folder: {e}");
        let log = fs::File::create(&log_file).map_err(folder)?;
        let mut cmd = Command::new(&self.exe);
        cmd.args(args)
            .arg("-case")
            .arg(work)
            .current_dir(work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(folder)?)
            .stderr(log);
        let status = supervise(&program, &mut cmd, limits, cancel, poll)?;
        if !status.success() {
            let log = fs::read_to_string(&log_file).unwrap_or_default();
            return Err(failure(args[0], status, &log, &log_file));
        }
        Ok(())
    }
}

/// Write the cell fields of the 1 file of `foamToVTK` in `work/VTK` to `step-1.vtu` and
/// `results.pvd` in `run_dir`.
pub(crate) fn write_result(work: &Path, run_dir: &Path) -> Result<(), String> {
    let vtk = work.join("VTK");
    let files: Vec<PathBuf> = fs::read_dir(&vtk)
        .map_err(|e| format!("{}: {e}", vtk.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "vtk"))
        .collect();
    let [file] = files.as_slice() else {
        return Err(format!(
            "{} holds {} .vtk files, expected 1",
            vtk.display(),
            files.len()
        ));
    };
    let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let frame = read_legacy(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    write_frames(run_dir, "unspecified", false, std::iter::once(frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VTK: &str = "# vtk DataFile Version 2.0\ncase name\nASCII\n\
        DATASET UNSTRUCTURED_GRID\nFIELD FieldData 1\nTimeValue 1 1 float\n2000\n\n\
        POINTS 8 float\n0 0 0 1 0 0 1 1 0 0 1 0\n0 0 1 1 0 1 1 1 1 0 1 1\n\
        CELLS 1 9\n8 0 1 2 3 4 5 6 7\nCELL_TYPES 1\n12\n\
        CELL_DATA 1\nFIELD FieldData 2\np 1 1 float\n-0.5\nU 3 1 float\n1 0.25 0\n";

    #[test]
    fn a_legacy_file_gives_the_time_the_cells_and_the_cell_fields() {
        let (time, grid) = read_legacy(VTK).unwrap();
        assert_eq!(time, 2000.0);
        assert_eq!(grid.points.len(), 8);
        assert_eq!(grid.points[6], [1.0, 1.0, 1.0]);
        assert_eq!(grid.cells, vec![vec![0, 1, 2, 3, 4, 5, 6, 7]]);
        assert_eq!(grid.cell_types, vec![12]);
        assert!(grid.point_data.is_empty());
        assert_eq!(grid.cell_field("p").unwrap().values.as_slice(), &[-0.5][..]);
        let u = grid.cell_field("U").unwrap();
        assert_eq!(
            (u.components, u.values.as_slice()),
            (3, &[1.0, 0.25, 0.0][..])
        );
    }

    #[test]
    fn a_damaged_legacy_file_is_an_error() {
        let short = &VTK[..VTK.find("CELL_TYPES").unwrap()];
        assert!(read_legacy(short).unwrap_err().contains("do not match"));
        let cut = &VTK[..VTK.len() - 4];
        assert!(read_legacy(cut).unwrap_err().contains("ends too early"));
        let bad_point = VTK.replace("8 0 1 2 3 4 5 6 7", "8 0 1 2 3 4 5 6 9");
        assert!(read_legacy(&bad_point)
            .unwrap_err()
            .contains("do not match"));
        let word = VTK.replace("-0.5", "x");
        assert!(read_legacy(&word).unwrap_err().contains("\"x\""));
    }

    #[test]
    fn the_application_comes_from_the_control_dict() {
        let dir = std::env::temp_dir().join(format!("fh-foam-app-{}", std::process::id()));
        fs::create_dir_all(dir.join("system")).unwrap();
        let dict = dir.join("system/controlDict");
        fs::write(
            &dict,
            "FoamFile {}\napplication     simpleFoam;\nendTime 10;\n",
        )
        .unwrap();
        assert_eq!(application(&dir).unwrap(), "simpleFoam");
        fs::write(&dict, "application $(rm -rf x);\n").unwrap();
        assert!(application(&dir)
            .unwrap_err()
            .contains("application <name>"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failure_shows_the_fatal_error_of_openfoam() {
        let log = "Create time\n\n--> FOAM FATAL IO ERROR: (openfoam-2512)\nkeyword div(phi,U) \
            is undefined\n\nfile: system/fvSchemes at line 20.\n\n    From Foam::dictionary\n\
            \nFOAM exiting\n";
        let status = Command::new("/bin/sh")
            .args(["-c", "exit 1"])
            .status()
            .unwrap();
        let text = failure("simpleFoam", status, log, Path::new("log.simpleFoam"));
        assert!(
            text.contains(
                "simpleFoam failed (exit status: 1): FOAM FATAL IO ERROR: (openfoam-2512) \
                 keyword div(phi,U) is undefined file: system/fvSchemes at line 20. Correct"
            ),
            "{text}"
        );
        assert!(text.contains("log.simpleFoam"), "{text}");
        let fpe = failure(
            "simpleFoam",
            status,
            "#0  Foam::sigFpe::sigHandler(int)",
            Path::new("l"),
        );
        assert!(fpe.contains("diverged"), "{fpe}");
    }
}
