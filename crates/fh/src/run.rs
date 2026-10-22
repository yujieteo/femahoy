//! `fh run`: solve a CalculiX input with CalculiX, an OpenFOAM case with OpenFOAM, a GetDP case
//! with GetDP, a preCICE case with OpenFOAM and CalculiX coupled by preCICE, a Chrono case with
//! Project Chrono, or a LAMMPS case with LAMMPS, or optimize the topology of a CalculiX model, and
//! show the progress.
//!
//! - `fh run MODEL.inp` gives CalculiX the input as it is, with each file that it includes
//!   (`*INCLUDE`). CalculiX is `$FH_CCX`, else `ccx` on `PATH`.
//! - `fh run CASE` gives OpenFOAM the case folder (`system/controlDict`, `constant`, `0`). See
//!   `fh::foam` for the programs that it starts. OpenFOAM is `$FH_OPENFOAM`, else `openfoam` on
//!   `PATH`.
//! - `fh run CASE` gives GetDP the case folder (`case.pro`, and `case.geo` or `case.msh`). See
//!   `fh::getdp` for the programs that it starts. GetDP is `$FH_GETDP`, else `getdp` on `PATH`.
//! - `fh run CASE` gives OpenFOAM and CalculiX the preCICE case folder (`precice-config.xml`,
//!   `fluid`, `solid`). See `fh::precice` for the programs that it starts. CalculiX with the
//!   adapter is `$FH_CCX_PRECICE`, else `ccx_preCICE` on `PATH`.
//! - `fh run CASE` gives LAMMPS the case folder (`case.lmp`). See `fh::lammps`. LAMMPS is
//!   `$FH_LAMMPS`, else `lmp` on `PATH`.
//! - `fh run CASE` optimizes the topology case folder (`job.inp` and `topology.toml`) with a loop
//!   of CalculiX solves. See `fh::topology`.
//! - `fh run CASE` gives Chrono the case folder (`chrono.py`, see `fh::chrono`). The Python of
//!   PyChrono is `$FH_PYCHRONO`, else `python3` on `PATH`.
//! - `fh run` solves a copy, so a later edit does not change the run.
//! - `--timeout SECONDS` and `--memory MIB` set the limits of the run.
//!
//! Each run has its own folder in the state folder (`$FH_STATE`, see `fh::daemon::new_run`): the
//! copy of the input, the solver files, and the VTU and PVD files. The page of `fh serve`
//! reads these folders. `fh run` needs no daemon.
//!
//! Before a CalculiX solve or a topology loop, `fh run` writes a preview folder, so that the page
//! shows the model with each support and each load.
//!
//! When `fh run` ends for any reason, the solver stops. Ctrl+C, SIGTERM and SIGHUP stop it and
//! end `fh run` with code 130. When `fh run` dies at once (SIGKILL), a guard process of the
//! solver kills it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fh::ccx::{result_frames, CancelToken, Ccx};
use fh::chrono::{self, Chrono};
use fh::daemon::{default_state_dir, new_preview, new_run};
use fh::foam::{self, Foam};
use fh::getdp::{self, GetDp};
use fh::lammps::{self, Lammps};
use fh::precice::{self, Precice};
use fh::topology;

const USAGE: &str = "usage: fh run MODEL.inp|CASE [--timeout SECONDS] [--memory MIB]\n\
--timeout has no limit by default. --memory is 85 % of the RAM by default. At a limit, fh run \
stops the solver. For a case, the limit counts all the programs.\n\
CASE is an OpenFOAM case folder with system/controlDict. fh run starts blockMesh (when the case \
has system/blockMeshDict and no constant/polyMesh), then setFields (when the case has \
system/setFieldsDict), then the application of system/controlDict, then foamToVTK.\n\
CASE is also a GetDP case folder with case.pro. fh run starts gmsh (when the case has case.geo), \
then getdp with the first Resolution and the first PostOperation of case.pro.\n\
CASE is also a preCICE case folder with precice-config.xml, the OpenFOAM case fluid and the \
CalculiX input solid/job.inp. fh run starts blockMesh (as for OpenFOAM), then the application \
of fluid and ccx_preCICE in solid at the same time, then foamToVTK.\n\
CASE is also a topology case folder with job.inp and topology.toml. fh run solves job.inp with \
ccx in each iteration of the loop.\n\
CASE is also a Chrono case folder with chrono.py. fh run starts python3 chrono.py, with the Python \
of PyChrono.\n\
CASE is also a LAMMPS case folder with case.lmp. fh run starts lmp -in case.lmp, then shows each \
snapshot of the text dump files (*.dump).";

/// Exit codes of `fh run`, in addition to 0 (the run succeeded) and 2 (usage error).
const FAILED: u8 = 1;
const CANCELLED: u8 = 130;

#[derive(Default)]
struct Options {
    target: Option<PathBuf>,
    timeout: Option<Duration>,
    /// The memory limit in bytes.
    memory: Option<u64>,
}

/// A number above 0 that is finite.
fn positive(value: &str, what: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| format!("{what} needs a number above 0\n{USAGE}"))
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |what: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs {what}\n{USAGE}"))
        };
        match arg.as_str() {
            "--timeout" => {
                let seconds = positive(&value("a number of seconds")?, "--timeout")?;
                o.timeout = Some(
                    Duration::try_from_secs_f64(seconds)
                        .map_err(|_| format!("--timeout is too large\n{USAGE}"))?,
                );
            }
            "--memory" => {
                let mib = positive(&value("a number of MiB")?, "--memory")?;
                o.memory = Some((mib * f64::from(1 << 20)) as u64);
            }
            s if s.starts_with('-') => return Err(format!("unknown option: {s}\n{USAGE}")),
            s if o.target.is_none() => o.target = Some(s.into()),
            s => return Err(format!("unexpected argument: {s}\n{USAGE}")),
        }
    }
    if o.target.is_none() {
        return Err(USAGE.into());
    }
    Ok(o)
}

/// The solver of a run.
#[derive(Clone, Copy, PartialEq)]
enum Solver {
    Ccx,
    OpenFoam,
    GetDp,
    Precice,
    Topology,
    Chrono,
    Lammps,
}

/// The real path of the `.inp` file or the case folder that `fh run` solves.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let shown = path.display();
    let full = fs::canonicalize(path).map_err(|e| format!("{shown}: {e}"))?;
    let inp = full
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("inp"));
    let case = foam::is_case(&full)
        || getdp::is_case(&full)
        || chrono::is_case(&full)
        || precice::is_case(&full)
        || lammps::is_case(&full)
        || topology::is_case(&full);
    if (full.is_file() && inp) || case {
        Ok(full)
    } else {
        Err(format!(
            "{shown}: fh run takes a CalculiX input file (.inp), an OpenFOAM case folder (with \
             system/controlDict), a GetDP case folder (with case.pro), a preCICE case folder \
             (with precice-config.xml), a Chrono case folder (with chrono.py), a LAMMPS case \
             folder (with case.lmp) or a topology case folder (with topology.toml)"
        ))
    }
}

fn fail(code: u8, message: &str) -> ExitCode {
    eprintln!("fh run: {message}");
    ExitCode::from(code)
}

/// Print the wall time and the result folder of a run that succeeded.
fn report(run: &str, out: &Path, seconds: f64, solver: Solver) {
    println!("fh run: {run} succeeded in {seconds:.2} s");
    println!("fh run: results in {}", out.display());
    let (results, no_field) = match solver {
        Solver::OpenFoam => {
            let work = out.join(foam::WORK).display().to_string();
            println!(
                "fh run: text results {work}/log.<program> (output of each program), \
                 {work}/postProcessing (function objects)"
            );
            return;
        }
        Solver::Precice => {
            let work = out.join(precice::WORK).display().to_string();
            println!(
                "fh run: text results {work}/<fluid|solid>/log.<program> (output of each \
                 program), {work}/fluid/postProcessing (function objects), {work}/solid \
                 (watch points of preCICE, ccx_preCICE files)"
            );
            return;
        }
        Solver::GetDp => (
            format!(
                "{0}/log.<program> (output of each program), the files that the PostOperation \
                 names in {0}",
                out.join(getdp::WORK).display()
            ),
            "Print a field to a .pos file in the PostOperation",
        ),
        Solver::Chrono => (
            format!(
                "{0}/log.python (output of chrono.py), the files that chrono.py names in {0}",
                out.join(chrono::WORK).display()
            ),
            "Write the motion to a .pos file with the view U in chrono.py",
        ),
        Solver::Lammps => (
            format!(
                "{0}/log.lmp (output of LAMMPS), the files that case.lmp writes in {0}",
                out.join(lammps::WORK).display()
            ),
            "Write a text dump in case.lmp: dump ID GROUP custom N particles.dump id x y z ...",
        ),
        Solver::Ccx => (
            format!(
                "{0}/ccx/job.dat (print requests), {0}/ccx/job.sta (steps)",
                out.display()
            ),
            "Add *NODE FILE or *EL FILE to a *STEP to write a field",
        ),
        Solver::Topology => (
            format!(
                "{0}/{1} (each iteration), {0}/solve/ccx/job.dat (the last solve)",
                out.join(topology::WORK).display(),
                topology::HISTORY
            ),
            "Add *NODE FILE or *EL FILE to the *STEP of job.inp to write a field",
        ),
    };
    println!("fh run: text results {results}");
    if result_frames(out) == 0 {
        println!("fh run: the page has no field to show. {no_field}");
    }
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let o = parse(args)?;
    let state =
        default_state_dir().ok_or_else(|| format!("no home folder, so set FH_STATE\n{USAGE}"))?;
    let file = resolve(o.target.as_deref().expect("parse needs a model"))?;
    let solver = if foam::is_case(&file) {
        Solver::OpenFoam
    } else if getdp::is_case(&file) {
        Solver::GetDp
    } else if precice::is_case(&file) {
        Solver::Precice
    } else if lammps::is_case(&file) {
        Solver::Lammps
    } else if topology::is_case(&file) {
        Solver::Topology
    } else if chrono::is_case(&file) {
        Solver::Chrono
    } else {
        Solver::Ccx
    };
    // The name of the model in the page: the input file.
    let title = file.file_name().unwrap_or_default().to_string_lossy();
    // The page shows a CalculiX model before the solve.
    let model = match solver {
        Solver::Topology => Some(file.join("job.inp")),
        Solver::Ccx => Some(file.clone()),
        _ => None,
    };
    if let Some(model) = model {
        match new_preview(&state, &model, &title) {
            Ok(preview) => println!(
                "fh run: preview: the page shows {title} (preview {preview}) with its supports \
                 and loads"
            ),
            Err(e) => return Ok(fail(FAILED, &format!("the preview failed: {e}"))),
        }
    }
    let run = match new_run(&state, &file) {
        Ok(run) => run,
        Err(e) => return Ok(fail(FAILED, &format!("the run folder failed: {e}"))),
    };
    println!("fh run: {} solves {}", run.id, file.display());
    let cancel = CancelToken::new();
    signal::install();
    let flag = cancel.clone();
    std::thread::spawn(move || {
        while !signal::received() {
            std::thread::sleep(Duration::from_millis(20));
        }
        flag.cancel();
    });
    let start = Instant::now();
    let on_program = &mut |program: &str| println!("  program {program}");
    let result = match solver {
        Solver::OpenFoam => {
            let mut foam = Foam::new();
            foam.timeout = o.timeout;
            foam.memory_limit = o.memory.unwrap_or(foam.memory_limit);
            foam.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::GetDp => {
            let mut getdp = GetDp::new();
            getdp.timeout = o.timeout;
            getdp.memory_limit = o.memory.unwrap_or(getdp.memory_limit);
            getdp.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Chrono => {
            let mut chrono = Chrono::new();
            chrono.timeout = o.timeout;
            chrono.memory_limit = o.memory.unwrap_or(chrono.memory_limit);
            chrono.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Precice => {
            let mut precice = Precice::new();
            precice.foam.timeout = o.timeout;
            precice.foam.memory_limit = o.memory.unwrap_or(precice.foam.memory_limit);
            precice.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Lammps => {
            let mut lammps = Lammps::new();
            lammps.timeout = o.timeout;
            lammps.memory_limit = o.memory.unwrap_or(lammps.memory_limit);
            lammps.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Ccx | Solver::Topology => {
            let mut ccx = Ccx::new();
            ccx.timeout = o.timeout.or(ccx.timeout);
            ccx.memory_limit = o.memory.unwrap_or(ccx.memory_limit);
            if solver == Solver::Topology {
                topology::run_with(&ccx, &run.input, &run.out, &cancel, &mut |i| {
                    println!(
                        "  iteration {} compliance {:.6e} volume {:.4} change {:.4}",
                        i.number, i.compliance, i.volume, i.change
                    );
                })
            } else {
                ccx.run_with(&run.input, &run.out, &cancel, &mut |p| {
                    println!("  progress step {} increment {}", p.step, p.increment);
                })
            }
        }
    };
    Ok(match result {
        Ok(()) => {
            report(&run.id, &run.out, start.elapsed().as_secs_f64(), solver);
            ExitCode::SUCCESS
        }
        Err(_) if cancel.is_cancelled() => fail(CANCELLED, &format!("{} was cancelled", run.id)),
        Err(e) => {
            // The message names the copy of the input. The person knows the input file.
            let message = e.replace(
                &run.input.display().to_string(),
                &file.display().to_string(),
            );
            fail(FAILED, &format!("{} failed: {message}", run.id))
        }
    })
}

/// SIGINT (Ctrl+C), SIGTERM and SIGHUP during a run. The handler only sets a flag. A thread
/// reads the flag and cancels the solve.
mod signal {
    use std::sync::atomic::{AtomicBool, Ordering};

    static RECEIVED: AtomicBool = AtomicBool::new(false);

    pub fn received() -> bool {
        RECEIVED.load(Ordering::SeqCst)
    }

    pub fn install() {
        extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        extern "C" fn on_signal(_: i32) {
            RECEIVED.store(true, Ordering::SeqCst);
        }
        // SIGHUP, SIGINT and SIGTERM have these numbers on Linux and macOS.
        for sig in [1, 2, 15] {
            // SAFETY: the handler only stores an atomic flag, which is async-signal-safe.
            unsafe {
                signal(sig, on_signal);
            }
        }
    }
}
