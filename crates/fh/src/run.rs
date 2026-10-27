//! `fh run`: solve a CalculiX input with CalculiX, an OpenFOAM case with OpenFOAM, a GetDP case
//! with GetDP, a preCICE case with OpenFOAM and CalculiX coupled by preCICE, a Chrono case with
//! Project Chrono, a LAMMPS case with LAMMPS, a Meep case with Meep, or an openEMS case with
//! openEMS, or optimize the topology or the shape of a CalculiX model, and show the progress.
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
//! - `fh run CASE` optimizes the shape case folder (`optimize.toml`, `mesh.geo` and `job.inp`)
//!   with a loop of gmsh meshes and CalculiX solves. See `fh::optimize`.
//! - `fh run CASE` gives Chrono the case folder (`chrono.py`, see `fh::chrono`). The Python of
//!   PyChrono is `$FH_PYCHRONO`, else `python3` on `PATH`.
//! - `fh run CASE` gives Meep the case folder (`meep_case.py`, see `fh::meep`). The Python of
//!   Meep is `$FH_MEEP`, else `python3` on `PATH`.
//! - `fh run CASE` gives openEMS the case folder (`openems.py`, see `fh::openems`). The Python of
//!   openEMS is `$FH_OPENEMS`, else `python3` on `PATH`.
//! - `fh run` solves a copy, so a later edit does not change the run.
//! - `--timeout SECONDS` sets the time limit of the run.
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
use fh::daemon::{default_state_dir, new_preview, new_run, start_solve};
use fh::foam::{self, Foam};
use fh::getdp::{self, GetDp};
use fh::lammps::{self, Lammps};
use fh::meep::{self, Meep};
use fh::openems::{self, OpenEms};
use fh::optimize;
use fh::precice::{self, Precice};
use fh::topology;
use fh::Solver;

const USAGE: &str = "usage: fh run MODEL.inp|CASE [--timeout SECONDS]\n\
--timeout has no limit by default. The memory limit is 85 % of the RAM. At a limit, fh run stops \
the solver. For a case, the limit counts all the programs.\n\
CASE is an OpenFOAM case folder with system/controlDict. fh run starts blockMesh (when the case \
has system/blockMeshDict and no constant/polyMesh, and blockMesh -region R for each \
system/R/blockMeshDict), then setFields (when the case has system/setFieldsDict), then the \
application of system/controlDict, then foamToVTK (with -allRegions when the case has \
constant/regionProperties).\n\
CASE is also a GetDP case folder with case.pro. fh run starts gmsh (when the case has case.geo), \
then getdp with the first Resolution and the first PostOperation of case.pro.\n\
CASE is also a preCICE case folder with precice-config.xml, the OpenFOAM case fluid and the \
CalculiX input solid/job.inp. fh run starts blockMesh (as for OpenFOAM), then the application \
of fluid and ccx_preCICE in solid at the same time, then foamToVTK.\n\
CASE is also a topology case folder with job.inp and topology.toml. fh run solves job.inp with \
ccx in each iteration of the loop.\n\
CASE is also a shape optimization case folder with optimize.toml, mesh.geo and job.inp. fh run \
starts gmsh and ccx for each design of the loop.\n\
CASE is also a Chrono case folder with chrono.py. fh run starts python3 chrono.py, with the Python \
of PyChrono.\n\
CASE is also a LAMMPS case folder with case.lmp. fh run starts lmp -in case.lmp, then shows each \
snapshot of the text dump files (*.dump).\n\
CASE is also a Meep case folder with meep_case.py. fh run starts python3 meep_case.py, with the \
Python of Meep.\n\
CASE is also an openEMS case folder with openems.py. fh run starts python3 openems.py, with the \
Python of openEMS.";

/// Exit codes of `fh run`, in addition to 0 (the run succeeded) and 2 (usage error).
const FAILED: u8 = 1;
const CANCELLED: u8 = 130;

#[derive(Default)]
struct Options {
    target: Option<PathBuf>,
    timeout: Option<Duration>,
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

/// The real path of the `.inp` file or the case folder that `fh run` solves.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let shown = path.display();
    let full = fs::canonicalize(path).map_err(|e| format!("{shown}: {e}"))?;
    let inp = full
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("inp"));
    if (full.is_file() && inp) || fh::solver(&full).is_some() {
        Ok(full)
    } else {
        Err(format!(
            "{shown}: fh run takes a CalculiX input file (.inp), an OpenFOAM case folder (with \
             system/controlDict), a GetDP case folder (with case.pro), a preCICE case folder \
             (with precice-config.xml), a Chrono case folder (with chrono.py), a LAMMPS case \
             folder (with case.lmp), a Meep case folder (with meep_case.py), an openEMS case \
             folder (with openems.py), a topology case folder (with topology.toml) or a shape \
             optimization case folder (with optimize.toml)"
        ))
    }
}

fn fail(code: u8, message: &str) -> ExitCode {
    eprintln!("fh run: {message}");
    ExitCode::from(code)
}

/// Print the wall time and the result folder of a run that succeeded. `topology` is the end of a
/// topology run.
fn report(
    run: &str,
    out: &Path,
    seconds: f64,
    solver: Solver,
    topology: Option<&topology::Outcome>,
) {
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
        Solver::Meep => (
            format!(
                "{0}/log.python (output of meep_case.py), the files that meep_case.py names in \
                 {0}",
                out.join(meep::WORK).display()
            ),
            "Write a field to a .pos file in meep_case.py",
        ),
        Solver::OpenEms => (
            format!(
                "{0}/log.python (output of openems.py), the files that openems.py names in {0}",
                out.join(openems::WORK).display()
            ),
            "Write a field to a .pos file in openems.py",
        ),
        Solver::Ccx => {
            let rms = out.join("ccx").join(fh::ccx::RMS_FILE);
            let rms = if rms.exists() {
                format!(", {} (RMS of a steady state step)", rms.display())
            } else {
                String::new()
            };
            (
                format!(
                    "{0}/ccx/job.dat (print requests), {0}/ccx/job.sta (steps){rms}",
                    out.display()
                ),
                "Add *NODE FILE or *EL FILE to a *STEP to write a field",
            )
        }
        Solver::Topology => {
            if let Some(end) = topology {
                print_cad(end);
            }
            (
                format!(
                    "{0}/{1} (each iteration), {0}/solve/ccx/job.dat (the last solve)",
                    out.join(topology::WORK).display(),
                    topology::HISTORY
                ),
                "Add *NODE FILE or *EL FILE to the *STEP of job.inp to write a field",
            )
        }
        Solver::Optimize => (
            format!(
                "{0}/{1} (each solve), {0}/{2} (the best design), {0}/solve/ccx/job.dat (the \
                 last solve)",
                out.join(optimize::WORK).display(),
                optimize::HISTORY,
                optimize::OPTIMUM
            ),
            "Add *NODE FILE or *EL FILE to the *STEP of job.inp to write a field",
        ),
    };
    println!("fh run: text results {results}");
    if result_frames(out) == 0 {
        println!("fh run: the page has no field to show. {no_field}");
    }
}

/// Print the STEP file of the CAD of a topology run and its check, or why the run has no CAD.
fn print_cad(end: &topology::Outcome) {
    let cad = match &end.cad {
        Ok(cad) => cad,
        Err(e) => return println!("fh run: no CAD: {e}"),
    };
    let step = cad.step.display();
    let solid = format!(
        "the solid of the density 0.5 or more, {} thick",
        cad.thickness
    );
    match &cad.check {
        Ok(check) => {
            let c = end.last.compliance;
            println!(
                "fh run: CAD {step} ({solid}). The check with ccx gives the compliance {:.6e} \
                 ({:+.2} % from {c:.6e} of the last design) and the volume fraction {:.4}",
                check.compliance,
                100.0 * (check.compliance - c) / c,
                check.volume
            );
        }
        Err(e) => println!("fh run: CAD {step} ({solid}). No check: {e}"),
    }
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let o = parse(args)?;
    let state =
        default_state_dir().ok_or_else(|| format!("no home folder, so set FH_STATE\n{USAGE}"))?;
    let file = resolve(o.target.as_deref().expect("parse needs a model"))?;
    let solver = fh::solver(&file).unwrap_or(Solver::Ccx);
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
    // The page shows the solve while it runs, with the solved fraction of a CalculiX solve.
    let periods = match solver {
        Solver::Ccx => fh_core::inp::flatten(&run.input)
            .ok()
            .and_then(|flat| {
                let steps = fh_core::inp::parse(&flat.text).steps;
                steps.iter().map(|s| s.period).collect::<Option<Vec<f64>>>()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let _solving = match start_solve(&run, &periods) {
        Ok(solving) => solving,
        Err(e) => return Ok(fail(FAILED, &e)),
    };
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
    let mut end = None;
    let on_program = &mut |program: &str| println!("  program {program}");
    let result = match solver {
        Solver::OpenFoam => {
            let mut foam = Foam::new();
            foam.timeout = o.timeout;
            foam.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::GetDp => {
            let mut getdp = GetDp::new();
            getdp.timeout = o.timeout;
            getdp.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Chrono => {
            let mut chrono = Chrono::new();
            chrono.timeout = o.timeout;
            chrono.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Meep => {
            let mut meep = Meep::new();
            meep.timeout = o.timeout;
            meep.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::OpenEms => {
            let mut openems = OpenEms::new();
            openems.timeout = o.timeout;
            openems.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Precice => {
            let mut precice = Precice::new();
            precice.foam.timeout = o.timeout;
            precice.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Lammps => {
            let mut lammps = Lammps::new();
            lammps.timeout = o.timeout;
            lammps.run_with(&run.input, &run.out, &cancel, on_program)
        }
        Solver::Ccx | Solver::Topology | Solver::Optimize => {
            let mut ccx = Ccx::new();
            ccx.timeout = o.timeout.or(ccx.timeout);
            if solver == Solver::Topology {
                topology::run_with(&ccx, &run.input, &run.out, &cancel, &mut |i| {
                    println!(
                        "  iteration {} compliance {:.6e} volume {:.4} change {:.4}",
                        i.number, i.compliance, i.volume, i.change
                    );
                })
                .map(|o| end = Some(o))
            } else if solver == Solver::Optimize {
                optimize::run_with(&ccx, &run.input, &run.out, &cancel, &mut |s| {
                    let values: Vec<String> = s
                        .variables
                        .iter()
                        .map(|(name, value)| format!("{name} {value:.6}"))
                        .collect();
                    println!(
                        "  solve {} {} objective {:.6e}",
                        s.number,
                        values.join(" "),
                        s.objective
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
            let seconds = start.elapsed().as_secs_f64();
            report(&run.id, &run.out, seconds, solver, end.as_ref());
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
