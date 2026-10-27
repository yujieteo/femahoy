//! The state folder on disk. `fh run` writes the run and preview folders. The daemon reads
//! them for the page.
//!
//! ```text
//! <state>/runs/<run>/source           the path of the input file or the case that the run solves
//! <state>/runs/<run>/in/input.inp     the read-only copy of that input, with the copies of its
//!                                     `*INCLUDE` files and of the global model of a `*SUBMODEL`
//! <state>/runs/<run>/in/system/ ...   or the read-only copy of an OpenFOAM case
//! <state>/runs/<run>/in/case.pro ...  or the read-only copy of a GetDP case
//! <state>/runs/<run>/in/topology.toml or the read-only copy of a topology case
//! <state>/runs/<run>/in/chrono.py ... or the read-only copy of a Chrono case
//! <state>/runs/<run>/in/case.lmp ...  or the read-only copy of a LAMMPS case
//! <state>/runs/<run>/in/optimize.toml or the read-only copy of an optimization case
//! <state>/runs/<run>/in/meep_case.py  or the read-only copy of a Meep case
//! <state>/runs/<run>/in/openems.py .. or the read-only copy of an openEMS case
//! <state>/runs/<run>/out/             run folder of the solver. `results.pvd` is the last
//!                                     file that a run writes, so a run has results when it lists
//!                                     a step
//! <state>/runs/<run>/solving          the start time of a solve that runs, and the step periods
//!                                     of a CalculiX input. `fh run` locks it during the solve and
//!                                     removes it at the end
//! <state>/previews/<n>/input.inp      the copy of the input of a preview, with its `*INCLUDE` files
//! <state>/previews/<n>/title          the name of the model in the page
//! ```
//!
//! A run ID or a preview number belongs to the process that creates its folder first, so
//! 2 `fh run` commands at the same time get 2 folders. A preview folder is built under a hidden
//! name and renamed, so the daemon never sees a partial preview. The daemon only reads these
//! folders, so any number of daemons can use one state folder.

use std::fs::{self, File, TryLockError};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ccx::{input_text, result_frames, stage_input, INPUT_FILE};
use crate::{foam, getdp, precice, topology, Solver};

use super::message::{bad, fail, Failure};

const RUNS_DIR: &str = "runs";
const PREVIEWS_DIR: &str = "previews";
const SOURCE_FILE: &str = "source";
const TITLE_FILE: &str = "title";
const IN_DIR: &str = "in";
const OUT_DIR: &str = "out";
const SOLVING_FILE: &str = "solving";

/// A run ID: `run-` and 6 or more digits. Run IDs count up in each state folder.
fn valid_run(id: &str) -> bool {
    id.strip_prefix("run-")
        .is_some_and(|n| n.len() >= 6 && n.bytes().all(|b| b.is_ascii_digit()))
}

fn run_id(number: u64) -> String {
    format!("run-{number:06}")
}

/// The folders of a new run.
pub struct Run {
    pub id: String,
    /// The read-only copy of the input file or the case. The run solves it.
    pub input: PathBuf,
    /// The run folder of the solver.
    pub out: PathBuf,
}

/// The highest number in the names of the folders of `dir`, or 0. `number` reads the number
/// of a name.
fn last_number(dir: &Path, number: impl Fn(&str) -> Option<u64>) -> u64 {
    let names = fs::read_dir(dir).into_iter().flatten().flatten();
    let numbers = names.filter_map(|e| number(e.file_name().to_str()?));
    numbers.max().unwrap_or(0)
}

/// Copy the input file, its `*INCLUDE` files and the global model of a `*SUBMODEL` into `dir`.
/// A missing include file or global model is a bad input.
fn stage(input: &Path, dir: &Path) -> Result<(), String> {
    stage_input(input, dir, INPUT_FILE)
}

/// Make the folder of a new run in `state` and copy `source` into it: an input file and its
/// `*INCLUDE` files, or an OpenFOAM, GetDP, preCICE, Chrono, LAMMPS, Meep, openEMS, topology or
/// optimization case. A later change of a file does not change the run.
pub fn new_run(state: &Path, source: &Path) -> Result<Run, String> {
    let runs = state.join(RUNS_DIR);
    let err = |e| format!("run folder: {e}");
    fs::create_dir_all(&runs).map_err(err)?;
    let mut number = last_number(&runs, |n| n.strip_prefix("run-")?.parse().ok()) + 1;
    let dir = loop {
        let dir = runs.join(run_id(number));
        match fs::create_dir(&dir) {
            Ok(()) => break dir,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => number += 1,
            Err(e) => return Err(err(e)),
        }
    };
    let input = match crate::solver(source) {
        Some(Solver::OpenFoam) => {
            foam::copy_case(source, &dir.join(IN_DIR), true)?;
            dir.join(IN_DIR)
        }
        Some(
            Solver::GetDp
            | Solver::Chrono
            | Solver::Lammps
            | Solver::Optimize
            | Solver::Meep
            | Solver::OpenEms,
        ) => {
            getdp::copy_case(source, &dir.join(IN_DIR), true)?;
            dir.join(IN_DIR)
        }
        Some(Solver::Topology) => {
            topology::copy_case(source, &dir.join(IN_DIR))?;
            dir.join(IN_DIR)
        }
        Some(Solver::Precice) => {
            precice::copy_case(source, &dir.join(IN_DIR), true)?;
            dir.join(IN_DIR)
        }
        Some(Solver::Ccx) | None => {
            stage(source, &dir.join(IN_DIR))?;
            dir.join(IN_DIR).join(INPUT_FILE)
        }
    };
    fs::write(
        dir.join(SOURCE_FILE),
        source.display().to_string().replace(char::is_control, " "),
    )
    .map_err(err)?;
    Ok(Run {
        id: run_id(number),
        input,
        out: dir.join(OUT_DIR),
    })
}

/// The seconds since 1970.
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// The `solving` file of a run while `fh run` solves it. The drop removes the file.
pub struct Solving {
    path: PathBuf,
    // The lock ends when the file closes.
    _file: File,
}

impl Drop for Solving {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Write and lock the `solving` file of `run`. `periods` are the time periods of the steps of
/// a CalculiX input, or empty.
pub fn start_solve(run: &Run, periods: &[f64]) -> Result<Solving, String> {
    let dir = run.out.parent().expect("a run folder holds out");
    let err = |e| format!("solve record: {e}");
    // Lock the file before the rename, so the daemon never sees it with no lock.
    let hidden = dir.join(format!(".{SOLVING_FILE}-{}", std::process::id()));
    let mut file = File::create(&hidden).map_err(err)?;
    file.lock().map_err(err)?;
    let periods: Vec<String> = periods.iter().map(f64::to_string).collect();
    write!(file, "started={}\nperiods={}\n", now(), periods.join(" ")).map_err(err)?;
    let path = dir.join(SOLVING_FILE);
    fs::rename(&hidden, &path).map_err(err)?;
    Ok(Solving { path, _file: file })
}

/// The solved fraction of a CalculiX solve: the step and the step time of the last line of
/// `job.sta` with the time period of each step.
fn solved_fraction(periods: &[f64], sta: &str) -> Option<f64> {
    let line = sta.lines().rev().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        let step = f.first()?.parse::<usize>().ok()?;
        f.get(1)?.parse::<u64>().ok()?;
        Some((step, f.get(5)?.parse::<f64>().ok()?))
    });
    let (step, time) = line?;
    let period = *periods.get(step.checked_sub(1)?)?;
    let total: f64 = periods.iter().sum();
    let done: f64 = periods[..step - 1].iter().sum::<f64>() + time.clamp(0.0, period);
    Some(done / total)
}

/// The last `n` bytes of a file, as text.
fn tail(path: &Path, n: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(n))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// A solve that runs now.
pub(crate) struct Solve {
    pub run: String,
    /// The folder of the input file, or the case folder.
    pub folder: String,
    pub elapsed: f64,
    /// The solved fraction of a CalculiX solve, from 0 to 1.
    pub fraction: Option<f64>,
}

/// Copy the input file and its `*INCLUDE` files with no run, so that the page shows the model
/// with its supports and loads before the solve. `title` is the name of the model in the
/// page. The answer is the preview number.
pub fn new_preview(state: &Path, input: &Path, title: &str) -> Result<String, String> {
    let previews = state.join(PREVIEWS_DIR);
    let err = |e| format!("preview folder: {e}");
    fs::create_dir_all(&previews).map_err(err)?;
    let hidden = previews.join(format!(".new-{}", std::process::id()));
    let _ = fs::remove_dir_all(&hidden);
    stage(input, &hidden)?;
    fs::write(
        hidden.join(TITLE_FILE),
        title.replace(char::is_control, " "),
    )
    .map_err(err)?;
    let mut number = last_number(&previews, |n| n.parse().ok()) + 1;
    loop {
        let dir = previews.join(number.to_string());
        match fs::rename(&hidden, &dir) {
            Ok(()) => return Ok(number.to_string()),
            // Another `fh run` took this number first.
            Err(_) if dir.exists() => number += 1,
            Err(e) => return Err(err(e)),
        }
    }
}

/// The text of a copy of an input, with its include files.
fn flat_text(path: &Path) -> Result<Vec<u8>, Failure> {
    input_text(path)
        .map(String::into_bytes)
        .map_err(|e| fail(500, e))
}

/// The folders of the state folder, for the daemon.
pub(crate) struct Store {
    dir: PathBuf,
}

impl Store {
    /// Make sure that the state folder has a `runs` folder. A daemon writes only the file
    /// `token` of the folder, once at the first start, so another daemon can use it at the same
    /// time.
    pub(crate) fn open(state_dir: &Path) -> Result<Store, String> {
        let err = |e| format!("state folder: {e}");
        fs::create_dir_all(state_dir.join(RUNS_DIR)).map_err(err)?;
        let dir = fs::canonicalize(state_dir).map_err(err)?;
        Ok(Store { dir })
    }

    fn run_dir(&self, run: &str) -> Result<PathBuf, Failure> {
        let dir = self.dir.join(RUNS_DIR).join(run);
        if valid_run(run) && dir.is_dir() {
            let canonical = dir.canonicalize().map_err(|e| fail(500, e.to_string()))?;
            if canonical.parent() != Some(self.dir.join(RUNS_DIR).as_path()) {
                return Err(fail(403, "run path leaves the retained runs"));
            }
            Ok(canonical)
        } else {
            Err(fail(404, format!("no run {run}")))
        }
    }

    /// The runs that have at least one result frame, in run order: the run and the path of its
    /// input file. The page cannot show a run with no frame, so it is not in the list.
    pub(crate) fn runs(&self) -> Result<Vec<(String, String)>, Failure> {
        let dir = self.dir.join(RUNS_DIR);
        let entries = fs::read_dir(&dir).map_err(|e| fail(500, format!("runs: {e}")))?;
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_run(n) && result_frames(&dir.join(n).join(OUT_DIR)) > 0)
            .collect();
        names.sort();
        Ok(names
            .into_iter()
            .map(|run| {
                let input = fs::read_to_string(dir.join(&run).join(SOURCE_FILE));
                (run, input.unwrap_or_default())
            })
            .collect())
    }

    /// The solves that run now, in run order. A `solving` file with no lock is not a solve.
    pub(crate) fn solves(&self) -> Vec<Solve> {
        let dir = self.dir.join(RUNS_DIR);
        let entries = fs::read_dir(&dir).into_iter().flatten().flatten();
        let mut names: Vec<String> = entries
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_run(n) && dir.join(n).join(SOLVING_FILE).is_file())
            .collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|run| {
                let path = dir.join(&run);
                let mut file = File::open(path.join(SOLVING_FILE)).ok()?;
                match file.try_lock_shared() {
                    Err(TryLockError::WouldBlock) => {}
                    _ => return None,
                }
                let mut text = String::new();
                file.read_to_string(&mut text).ok()?;
                let field = |k: &str| {
                    let prefix = format!("{k}=");
                    text.lines()
                        .find_map(|l| l.strip_prefix(&prefix).map(str::to_owned))
                };
                let started: f64 = field("started")?.parse().ok()?;
                let periods: Vec<f64> = field("periods")?
                    .split_whitespace()
                    .filter_map(|p| p.parse().ok())
                    .collect();
                let sta = path.join(OUT_DIR).join("ccx").join("job.sta");
                let fraction = tail(&sta, 4096).and_then(|t| solved_fraction(&periods, &t));
                let source = PathBuf::from(fs::read_to_string(path.join(SOURCE_FILE)).ok()?);
                let folder = if source.is_dir() {
                    source
                } else {
                    source.parent().map(Path::to_owned).unwrap_or(source)
                };
                Some(Solve {
                    run,
                    folder: folder.display().to_string(),
                    elapsed: (now() - started).max(0.0),
                    fraction,
                })
            })
            .collect()
    }

    /// List only retained files of this run. Never accept a workspace path from the page.
    pub(crate) fn files(&self, run: &str) -> Result<Vec<String>, Failure> {
        let dir = self.run_dir(run)?;
        let mut files = Vec::new();
        for (index, path) in self.input_files(run)?.iter().enumerate() {
            let name = path
                .strip_prefix(dir.join(IN_DIR))
                .unwrap_or(path)
                .display()
                .to_string()
                .replace(char::is_control, " ");
            files.push(format!("inputs/{index} {name}"));
        }
        let out = dir.join(OUT_DIR);
        if out.is_symlink() {
            return Err(fail(403, "result path leaves the run"));
        }
        for entry in fs::read_dir(&out).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // A hidden file, such as the run marker `.fh-run`, is not a result for the person.
            if fh_viewer::results::is_plain_file_name(&name)
                && !name.starts_with('.')
                && entry.file_type().is_ok_and(|t| t.is_file())
            {
                files.push(format!("files/{name}"));
            }
        }
        if dir.join("report.html").is_file() {
            files.push("report".to_owned());
        }
        files.sort();
        Ok(files)
    }

    /// Enumerate retained input files, with no symbolic link or workspace path.
    fn input_files(&self, run: &str) -> Result<Vec<PathBuf>, Failure> {
        fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    visit(&entry.path(), files)?;
                } else if kind.is_file() {
                    files.push(entry.path());
                }
            }
            Ok(())
        }
        let root = self.run_dir(run)?.join(IN_DIR);
        if root.is_symlink() {
            return Err(fail(403, "input path leaves the run"));
        }
        let mut files = Vec::new();
        if root.is_dir() {
            visit(&root, &mut files).map_err(|e| fail(500, e.to_string()))?;
        }
        files.sort();
        Ok(files)
    }

    pub(crate) fn retained_input(&self, run: &str, index: &str) -> Result<Vec<u8>, Failure> {
        let index: usize = index
            .parse()
            .map_err(|_| bad("input file index is not a number"))?;
        let files = self.input_files(run)?;
        let path = files
            .get(index)
            .ok_or_else(|| fail(404, "no such retained input file"))?;
        let canonical = path.canonicalize().map_err(|e| fail(404, e.to_string()))?;
        if !canonical.starts_with(self.run_dir(run)?.join(IN_DIR)) {
            return Err(fail(403, "input path leaves the run"));
        }
        fs::read(canonical).map_err(|e| fail(500, e.to_string()))
    }

    pub(crate) fn report_file(&self, run: &str) -> Result<Vec<u8>, Failure> {
        let dir = self.run_dir(run)?;
        let path = dir
            .join("report.html")
            .canonicalize()
            .map_err(|_| fail(404, "No report for this run"))?;
        let root = dir.canonicalize().map_err(|_| fail(404, "no run"))?;
        if !path.starts_with(root) {
            return Err(fail(403, "report path leaves the run"));
        }
        fs::read(path).map_err(|e| fail(500, e.to_string()))
    }

    /// The run has a retained report.
    pub(crate) fn has_report(&self, run: &str) -> bool {
        self.run_dir(run)
            .is_ok_and(|dir| dir.join("report.html").is_file())
    }

    /// Save a self-contained report inside each run. The record contains no source path or token.
    pub(crate) fn register_report(&self, runs: &[String], html: &[u8]) -> Result<(), String> {
        for run in runs {
            let dir = self.run_dir(run).map_err(|(_, e)| e)?;
            let temporary = dir.join(format!(".report-{}", super::random_hex(8)?));
            fs::write(&temporary, html).map_err(|e| e.to_string())?;
            fs::rename(&temporary, dir.join("report.html")).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// The previews, in the order of their numbers: the number and the title.
    pub(crate) fn previews(&self) -> Vec<(String, String)> {
        let dir = self.dir.join(PREVIEWS_DIR);
        let entries = fs::read_dir(&dir).into_iter().flatten().flatten();
        let mut numbers: Vec<u64> = entries
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .collect();
        numbers.sort_unstable();
        numbers
            .into_iter()
            .map(|n| {
                let title = fs::read_to_string(dir.join(n.to_string()).join(TITLE_FILE));
                (n.to_string(), title.unwrap_or_default())
            })
            .collect()
    }

    /// The text of the input of a preview, for the page. It holds the text of the include files.
    pub(crate) fn preview_file(&self, id: &str) -> Result<Vec<u8>, Failure> {
        let unknown = || fail(404, format!("no preview {id:?}"));
        let n: u64 = id.parse().map_err(|_| unknown())?;
        let path = self
            .dir
            .join(PREVIEWS_DIR)
            .join(n.to_string())
            .join(INPUT_FILE);
        if !path.is_file() {
            return Err(unknown());
        }
        flat_text(&path)
    }

    /// The text of the copy of the input that a run solves, for the page. It holds the text of
    /// the include files. An OpenFOAM, GetDP, preCICE, Chrono, LAMMPS, Meep, openEMS or
    /// optimization case has no input text for the page, so it gives no bytes. A topology case
    /// gives its `job.inp`.
    pub(crate) fn input_file(&self, run: &str) -> Result<Vec<u8>, Failure> {
        let input = self.run_dir(run)?.join(IN_DIR);
        match crate::solver(&input) {
            Some(Solver::Topology) => flat_text(&input.join("job.inp")),
            Some(_) => Ok(Vec::new()),
            None => flat_text(&input.join(INPUT_FILE)),
        }
    }

    /// The bytes of one file in the run folder of a run, for the viewer. `name` is a plain
    /// file name: letters, digits, `.`, `_` and `-`, with no `.` first.
    pub(crate) fn result_file(&self, run: &str, name: &str) -> Result<Vec<u8>, Failure> {
        let plain = (1..=128).contains(&name.len())
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if !plain {
            return Err(bad(format!("{name:?} is not a plain file name")));
        }
        let root = self.run_dir(run)?.join(OUT_DIR);
        if root.is_symlink() {
            return Err(fail(403, "result path leaves the run"));
        }
        let path = root.join(name).canonicalize().map_err(|e| match e.kind() {
            ErrorKind::NotFound => fail(404, format!("run {run} has no file {name}")),
            _ => fail(500, format!("{name}: {e}")),
        })?;
        if !path.starts_with(root) {
            return Err(fail(403, "result path leaves the run"));
        }
        fs::read(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => fail(404, format!("run {run} has no file {name}")),
            _ => fail(500, format!("{name}: {e}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::solved_fraction;

    #[test]
    fn the_fraction_counts_the_earlier_steps() {
        let sta = " SUMMARY OF JOB INFORMATION\n  STEP      INC     ATT  ITRS     TOT TIME     \
                   STEP TIME      INC TIME\n     1          4     1     2  0.100000E+01  \
                   0.100000E+01  0.125000E+00\n     2          3     1     2  0.100300E+01  \
                   0.300000E-02  0.100000E-02\n";
        let f = solved_fraction(&[1.0, 0.012], sta).unwrap();
        assert!((f - 1.003 / 1.012).abs() < 1e-12, "{f}");
        assert_eq!(solved_fraction(&[], sta), None, "no periods");
        assert_eq!(
            solved_fraction(&[1.0], sta),
            None,
            "a step that the input has not"
        );
        assert_eq!(
            solved_fraction(&[1.0], " SUMMARY OF JOB INFORMATION\n"),
            None
        );
    }
}
