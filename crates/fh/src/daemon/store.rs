//! The state folder on disk. `fh run` writes the run and preview folders. The daemon reads
//! them for the page.
//!
//! ```text
//! <state>/runs/<run>/source           the path of the input file or the case that the run solves
//! <state>/runs/<run>/in/input.inp     the read-only copy of that input, with the copies of its
//!                                     `*INCLUDE` files
//! <state>/runs/<run>/in/system/ ...   or the read-only copy of an OpenFOAM case
//! <state>/runs/<run>/out/             run folder of ccx or OpenFOAM. `results.pvd` is the last
//!                                     file that a run writes, so a run has results when it lists
//!                                     a step
//! <state>/previews/<n>/input.inp      the copy of the input of a preview, with its `*INCLUDE` files
//! <state>/previews/<n>/title          the name of the model in the page
//! ```
//!
//! A run ID or a preview number belongs to the process that creates its folder first, so
//! 2 `fh run` commands at the same time get 2 folders. A preview folder is built under a hidden
//! name and renamed, so the daemon never sees a partial preview. The daemon only reads these
//! folders, so any number of daemons can use one state folder.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::ccx::{input_text, result_frames, stage_input};
use crate::foam;

use super::message::{bad, fail, Failure};

const RUNS_DIR: &str = "runs";
const PREVIEWS_DIR: &str = "previews";
const SOURCE_FILE: &str = "source";
const TITLE_FILE: &str = "title";
const INPUT_FILE: &str = "input.inp";
const IN_DIR: &str = "in";
const OUT_DIR: &str = "out";

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
    /// The run folder of ccx or OpenFOAM.
    pub out: PathBuf,
}

/// The highest number in the names of the folders of `dir`, or 0. `number` reads the number
/// of a name.
fn last_number(dir: &Path, number: impl Fn(&str) -> Option<u64>) -> u64 {
    let names = fs::read_dir(dir).into_iter().flatten().flatten();
    let numbers = names.filter_map(|e| number(e.file_name().to_str()?));
    numbers.max().unwrap_or(0)
}

/// Copy the input file and its `*INCLUDE` files into `dir`. A missing include file is a bad input.
fn stage(input: &Path, dir: &Path) -> Result<(), String> {
    stage_input(input, dir, INPUT_FILE)
}

/// Make the folder of a new run in `state` and copy `source` into it: an input file and its
/// `*INCLUDE` files, or an OpenFOAM case. A later change of a file does not change the run.
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
    let input = if foam::is_case(source) {
        foam::copy_case(source, &dir.join(IN_DIR), true)?;
        dir.join(IN_DIR)
    } else {
        stage(source, &dir.join(IN_DIR))?;
        dir.join(IN_DIR).join(INPUT_FILE)
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
    /// Make sure that the state folder has a `runs` folder. A daemon changes no file of the
    /// folder, so another daemon can use it at the same time.
    pub(crate) fn open(state_dir: &Path) -> Result<Store, String> {
        let err = |e| format!("state folder: {e}");
        fs::create_dir_all(state_dir.join(RUNS_DIR)).map_err(err)?;
        let dir = fs::canonicalize(state_dir).map_err(err)?;
        Ok(Store { dir })
    }

    fn run_dir(&self, run: &str) -> Result<PathBuf, Failure> {
        let dir = self.dir.join(RUNS_DIR).join(run);
        if valid_run(run) && dir.is_dir() {
            Ok(dir)
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
    /// the include files. An OpenFOAM case has no input text for the page, so it gives no bytes.
    pub(crate) fn input_file(&self, run: &str) -> Result<Vec<u8>, Failure> {
        let input = self.run_dir(run)?.join(IN_DIR);
        if foam::is_case(&input) {
            return Ok(Vec::new());
        }
        flat_text(&input.join(INPUT_FILE))
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
        let path = self.run_dir(run)?.join(OUT_DIR).join(name);
        fs::read(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => fail(404, format!("run {run} has no file {name}")),
            _ => fail(500, format!("{name}: {e}")),
        })
    }
}
