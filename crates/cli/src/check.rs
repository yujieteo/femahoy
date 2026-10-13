//! `fh check [--root DIR] [FILE.inp...]`: parse `.inp` files with the `inp` crate.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Every `.inp` file under `dir`. A missing `dir` gives no files. Any other read error fails.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    for entry in entries {
        let path = entry
            .map_err(|e| format!("cannot read an entry of {}: {e}", dir.display()))?
            .path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|x| x == "inp") {
            out.push(path);
        }
    }
    Ok(())
}

/// The fixtures of a repository: `fixtures/` and `validation/cases/`, sorted.
pub fn fixture_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect(&root.join("fixtures"), &mut files)?;
    collect(&root.join("validation/cases"), &mut files)?;
    files.sort();
    Ok(files)
}

/// True for a file inside `validation/cases`. The solver needs a section there.
/// The test uses the real location of the file, thus a relative path gives the same result.
fn is_case_mesh(path: &Path) -> bool {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let names: Vec<_> = real.components().map(|c| c.as_os_str()).collect();
    names
        .windows(2)
        .any(|w| w[0] == "validation" && w[1] == "cases")
}

/// The first element without a `*SOLID SECTION`, as an error text.
fn section_problem(model: &fh_inp::Model) -> Option<String> {
    let covered: std::collections::HashSet<u32> = model
        .sections
        .iter()
        .filter_map(|s| model.elset(&s.elset))
        .flat_map(|set| set.ids.iter().copied())
        .collect();
    model
        .elements
        .iter()
        .find(|e| !covered.contains(&e.id))
        .map(|e| format!("element {} has no *SOLID SECTION", e.id))
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut root = PathBuf::from(".");
    let mut files: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = it.next().ok_or("--root needs a directory")?.into(),
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            s => files.push(s.into()),
        }
    }
    if files.is_empty() {
        files = fixture_files(&root)?;
        if files.is_empty() {
            return Err(format!("no .inp fixtures found under {}", root.display()));
        }
    }
    let mut failed = 0;
    for file in &files {
        let shown = file.display();
        let result = std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                fh_inp::parse_file(&shown.to_string(), &text).map_err(|e| fh_inp::join_errors(&e))
            })
            .and_then(|m| match is_case_mesh(file) {
                true => section_problem(&m).map_or(Ok(m), Err),
                false => Ok(m),
            });
        match result {
            Ok(m) => println!(
                "ok\t{shown}\t{} nodes, {} elements, {} steps",
                m.nodes.len(),
                m.elements.len(),
                m.steps.len()
            ),
            Err(e) => {
                failed += 1;
                eprintln!("fail\t{shown}\t{e}");
            }
        }
    }
    println!(
        "{} files: {} ok, {failed} failed",
        files.len(),
        files.len() - failed
    );
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
