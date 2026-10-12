//! `fh check [--root DIR] [FILE.inp...]`: parse `.inp` files with the `inp` crate.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Every `.inp` file under `dir`, sorted. A missing `dir` gives no files.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|x| x == "inp") {
            out.push(path);
        }
    }
}

/// The fixtures of a repository: `fixtures/` and `validation/cases/`.
pub fn fixture_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(&root.join("fixtures"), &mut files);
    collect(&root.join("validation/cases"), &mut files);
    files.sort();
    files
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
        files = fixture_files(&root);
        if files.is_empty() {
            return Err(format!("no .inp fixtures found under {}", root.display()));
        }
    }
    let mut failed = 0;
    for file in &files {
        let shown = file.display();
        let result = std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|text| fh_inp::parse(&text).map_err(|e| e.to_string()));
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
