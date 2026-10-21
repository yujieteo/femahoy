//! preCICE as the coupling of OpenFOAM (the fluid) and CalculiX (the solid) for fluid-structure
//! interaction, in the process of `fh`.
//!
//! A preCICE case is a folder with:
//!
//! - `precice-config.xml`, the coupling, with the solid participant `Solid`;
//! - `fluid`, an OpenFOAM case (see [`crate::foam`]) with `system/preciceDict` for the OpenFOAM
//!   adapter of preCICE;
//! - `solid`, the CalculiX input `job.inp` with `config.yml` for the CalculiX adapter of preCICE.
//!
//! [`Precice::run`] copies the case to `<run-dir>/precice` and starts there:
//!
//! 1. `blockMesh` in `fluid`, when it has `system/blockMeshDict` and no `constant/polyMesh`;
//! 2. at the same time, the application of `fluid/system/controlDict` in `fluid`, and the user's
//!    `ccx_preCICE` (`$FH_CCX_PRECICE`, else `ccx_preCICE` on PATH) in `solid`:
//!    `ccx_preCICE -i job -precice-participant Solid`;
//! 3. `foamToVTK` in `fluid` for the last time.
//!
//! Each program writes its output to `log.<program>` in its folder. preCICE stops the 2 programs
//! of step 2 at the `max-time` of the coupling. When one of them fails, the run stops the other.
//! When one of them ends and the other does not end in the same time again (at least 1 minute),
//! for example because it does not couple, the run stops it. Then the run writes the cell fields
//! of the fluid at the last time to `step-1.vtu` and `results.pvd`, as for an OpenFOAM case.
//!
//! preCICE is LGPL, and OpenFOAM, CalculiX and the adapters are GPL, so femahoy only starts them
//! as separate programs. A run has the limits of a run of OpenFOAM. The 2 programs of step 2
//! have half of the memory limit each.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ccx::{log_tail, prepare_run_dir, supervise, CancelToken, Program, CANCELLED};
use crate::foam::{self, Foam};
use crate::getdp;

/// The folder of the preCICE files in a run folder.
pub const WORK: &str = "precice";
/// The folders of the 2 participants in a case.
const FLUID: &str = "fluid";
const SOLID: &str = "solid";
/// CalculiX with the adapter.
const SOLVER: &str = "ccx_preCICE";
/// The shortest time that a program has to end after the other program ended.
const GRACE: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 60 });

/// True when `dir` is a preCICE case: it has `precice-config.xml`.
pub fn is_case(dir: &Path) -> bool {
    dir.join("precice-config.xml").is_file()
}

/// Copy the files of the case `case`, its OpenFOAM case `fluid` and the files of `solid` to
/// `dir`. With `read_only`, each copied file is read-only.
pub(crate) fn copy_case(case: &Path, dir: &Path, read_only: bool) -> Result<(), String> {
    getdp::copy_case(case, dir, read_only)?;
    foam::copy_case(&case.join(FLUID), &dir.join(FLUID), read_only)?;
    getdp::copy_case(&case.join(SOLID), &dir.join(SOLID), read_only)
}

/// The first error of preCICE (`ERROR:`) or CalculiX (`*ERROR`) in `log`, from the word `ERROR`
/// on, with no color codes.
pub(crate) fn error_line(log: &str) -> Option<String> {
    let line = log.lines().find(|l| l.contains("ERROR"))?;
    let mut text = String::new();
    let mut chars = line[line.find("ERROR")?..].chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // A color code: ESC, `[`, numbers and a letter.
            chars.by_ref().find(char::is_ascii_alphabetic);
        } else {
            text.push(c);
        }
    }
    Some(text.trim().to_owned())
}

/// The programs of a coupled run with their limits.
#[derive(Debug, Clone)]
pub struct Precice {
    /// `$FH_CCX_PRECICE`, else `ccx_preCICE` on PATH.
    pub ccx: PathBuf,
    /// OpenFOAM, with the time and memory limits of a run.
    pub foam: Foam,
}

impl Default for Precice {
    fn default() -> Self {
        Self::new()
    }
}

impl Precice {
    /// `ccx_preCICE` from `$FH_CCX_PRECICE`, else `ccx_preCICE` on PATH, and [`Foam::new`].
    pub fn new() -> Self {
        Precice {
            ccx: std::env::var_os("FH_CCX_PRECICE").map_or_else(|| SOLVER.into(), PathBuf::from),
            foam: Foam::new(),
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
        if !is_case(&case)
            || !foam::is_case(&case.join(FLUID))
            || !case.join(SOLID).join("job.inp").is_file()
        {
            return Err(format!(
                "{} is not a preCICE case: it needs precice-config.xml, fluid/system/controlDict \
                 and solid/job.inp",
                case.display()
            ));
        }
        let app = foam::application(&case.join(FLUID))?;
        let run_dir = prepare_run_dir(run_dir, &case)?;
        let work = run_dir.join(WORK);
        copy_case(&case, &work, false)?;
        let (fluid, solid) = (work.join(FLUID), work.join(SOLID));
        let limits = (start, self.foam.timeout, self.foam.memory_limit);
        if fluid.join("system/blockMeshDict").is_file() && !fluid.join("constant/polyMesh").exists()
        {
            on_program("blockMesh");
            self.foam
                .program(&["blockMesh"], &fluid, limits, cancel, &mut || {})?;
        }
        on_program(&app);
        on_program(SOLVER);
        self.couple(&app, &fluid, &solid, start, cancel)?;
        on_program(foam::TO_VTK[0]);
        self.foam
            .program(&foam::TO_VTK, &fluid, limits, cancel, &mut || {})?;
        foam::write_result(&fluid, &run_dir)
    }

    /// Run the fluid application `app` in `fluid` and `ccx_preCICE` in `solid` at the same time.
    /// The error is the first error in time: the other program then stops.
    fn couple(
        &self,
        app: &str,
        fluid: &Path,
        solid: &Path,
        start: Instant,
        cancel: &CancelToken,
    ) -> Result<(), String> {
        let memory = match self.foam.memory_limit {
            u64::MAX => u64::MAX,
            limit => limit / 2,
        };
        let limits = (start, self.foam.timeout, memory);
        // `stop` stops both programs. `first_end` is the end of the first program.
        let stop = CancelToken::new();
        let first_end: Mutex<Option<Instant>> = Mutex::new(None);
        let watch = || {
            let late = first_end
                .lock()
                .unwrap()
                .is_some_and(|end| end.elapsed() > (end - start).max(GRACE));
            if cancel.is_cancelled() || late {
                stop.cancel();
            }
        };
        let ended = |name: &str, result: Result<(), String>| {
            let now = Instant::now();
            first_end.lock().unwrap().get_or_insert(now);
            if result.is_err() {
                stop.cancel();
            }
            (now, name.to_owned(), result)
        };
        let mut ends = std::thread::scope(|s| {
            let fluid_end = s.spawn(|| {
                let result = self
                    .foam
                    .program(&[app], fluid, limits, &stop, &mut || watch());
                ended(app, result)
            });
            let solid_end = s.spawn(|| {
                let result = self.solid(solid, limits, &stop, &mut || watch());
                ended(SOLVER, result)
            });
            [fluid_end.join(), solid_end.join()].map(|end| {
                end.unwrap_or_else(|_| (Instant::now(), "a program".into(), Err("panic".into())))
            })
        });
        ends.sort_by_key(|(time, ..)| *time);
        let names = [ends[0].1.clone(), ends[1].1.clone()];
        match ends
            .into_iter()
            .find_map(|(_, name, r)| r.err().map(|e| (name, e)))
        {
            None => Ok(()),
            Some((name, e)) if e == CANCELLED && !cancel.is_cancelled() => {
                let other = if name == names[0] {
                    &names[1]
                } else {
                    &names[0]
                };
                Err(format!(
                    "{name} did not end after {other} ended, so the run stopped it. Both \
                     programs must couple through preCICE: the log of each program shows the \
                     preCICE lines"
                ))
            }
            Some((_, e)) => Err(e),
        }
    }

    /// Run `ccx_preCICE` on `solid/job.inp`, with its output in `solid/log.ccx_preCICE`.
    fn solid(
        &self,
        solid: &Path,
        limits: (Instant, Option<Duration>, u64),
        cancel: &CancelToken,
        poll: &mut dyn FnMut(),
    ) -> Result<(), String> {
        let program = Program {
            name: SOLVER,
            install: "Install preCICE and its CalculiX adapter (README.md) and put ccx_preCICE \
                      on PATH, or set FH_CCX_PRECICE",
        };
        let log_file = solid.join(format!("log.{SOLVER}"));
        let folder = |e: std::io::Error| format!("run folder: {e}");
        let log = fs::File::create(&log_file).map_err(folder)?;
        let mut cmd = Command::new(&self.ccx);
        cmd.args(["-i", "job", "-precice-participant", "Solid"])
            .current_dir(solid)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(folder)?)
            .stderr(log);
        let status = supervise(&program, &mut cmd, limits, cancel, poll)?;
        if status.success() {
            return Ok(());
        }
        let log = fs::read_to_string(&log_file).unwrap_or_default();
        let see = format!("The full output is in {}", log_file.display());
        Err(match error_line(&log) {
            Some(e) => {
                format!("{SOLVER} failed ({status}): {e}. Correct the case as the message tells. {see}.")
            }
            None => format!(
                "{SOLVER} failed ({status}). {see}. Its last lines: {}",
                log_tail(&log)
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn an_error_line_has_no_color_codes() {
        let log = "Set ID Found\n(0) 22:26:24 [xml::XMLParser]:176 in readFileContent: \
            \u{1b}[31mERROR: \u{1b}[0mXML parser was unable to open configuration file \
            \"../precice-config.xml\"\nmore\n";
        assert_eq!(
            error_line(log).unwrap(),
            "ERROR: XML parser was unable to open configuration file \"../precice-config.xml\""
        );
        assert_eq!(
            error_line(" *ERROR: too many cutbacks\n").unwrap(),
            "ERROR: too many cutbacks"
        );
        assert_eq!(error_line("Job finished\n"), None);
    }

    /// A case with the fake programs `openfoam` and `ccx_preCICE` in `dir`: `fluid` and `solid`
    /// are the shell lines of the 2 coupled programs.
    fn lab(name: &str, fluid: &str, solid: &str) -> (PathBuf, Precice) {
        let dir = std::env::temp_dir().join(format!("fh-precice-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let case = dir.join("case");
        fs::create_dir_all(case.join("fluid/system")).unwrap();
        fs::create_dir_all(case.join("solid")).unwrap();
        fs::write(
            case.join("precice-config.xml"),
            "<precice-configuration/>\n",
        )
        .unwrap();
        fs::write(
            case.join("fluid/system/controlDict"),
            "application pimpleFoam;\n",
        )
        .unwrap();
        fs::write(case.join("solid/job.inp"), "*HEADING\n").unwrap();
        let script = |file: &str, text: String| {
            let path = dir.join(file);
            fs::write(&path, format!("#!/bin/sh\n{text}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let openfoam = script(
            "openfoam",
            format!("[ \"$1\" = pimpleFoam ] && {{ {fluid}; }}\nexit 0"),
        );
        let ccx = script("ccx_preCICE", solid.to_owned());
        let mut precice = Precice::new();
        precice.ccx = ccx;
        precice.foam.exe = openfoam;
        (dir, precice)
    }

    #[test]
    fn a_failed_solid_stops_the_fluid_and_fails_the_run_with_its_error() {
        let (dir, precice) = lab(
            "solid",
            "sleep 30",
            "echo ' *ERROR reading *CLOAD: node set NINTERFACE does not exist'; exit 201",
        );
        let start = Instant::now();
        let e = precice
            .run(&dir.join("case"), &dir.join("run"))
            .unwrap_err();
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the fluid did not stop"
        );
        assert!(
            e.starts_with(
                "ccx_preCICE failed (exit status: 201): ERROR reading *CLOAD: node set \
                 NINTERFACE does not exist."
            ),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_program_that_does_not_end_after_the_other_stops() {
        let (dir, precice) = lab("late", "exit 0", "sleep 30");
        let start = Instant::now();
        let e = precice
            .run(&dir.join("case"), &dir.join("run"))
            .unwrap_err();
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the solid did not stop"
        );
        assert!(
            e.starts_with("ccx_preCICE did not end after pimpleFoam ended"),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_without_the_participants_is_not_a_case() {
        let (dir, precice) = lab("parts", "exit 0", "exit 0");
        fs::remove_file(dir.join("case/solid/job.inp")).unwrap();
        let e = precice
            .run(&dir.join("case"), &dir.join("run"))
            .unwrap_err();
        assert!(e.contains("is not a preCICE case"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }
}
