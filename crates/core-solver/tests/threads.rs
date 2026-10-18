//! The result files do not depend on the number of threads. The element loops and the
//! conjugate gradient solver run on 1, 2, 4 and all cores of the machine, and every file of
//! the run folder must have the same bytes. Decision D-10, see DESIGN.md.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{assert_same, block_input, snapshot};
use fh_core_solver::{input_hash, run_model, Formats, Input, Method, Options};

/// A cube of 14 x 14 x 14 C3D8 elements with 2 steps. It has 9450 free dofs, so the conjugate
/// gradient solver has 10 blocks of 1024 rows, and a team of 10 threads gives one to each.
fn cube() -> String {
    block_input([14, 14, 14], [1.0, 1.0, 1.0], 2)
}

/// A bar of 32 x 4 x 4 C3D8 elements with 2 steps and 2400 free dofs. A debug build
/// factors it in a short time.
fn bar() -> String {
    block_input([32, 4, 4], [8.0, 1.0, 1.0], 2)
}

/// Run `text` with `threads` (0 is all cores). Returns the run folder and the step lines.
fn run(text: &str, name: &str, method: Method, threads: usize) -> (PathBuf, Vec<String>) {
    let d = std::env::temp_dir().join(format!(
        "fh-threads-{name}-{threads}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&d);
    let model = fh_inp::parse(text).unwrap();
    let input = Input {
        name: "block.inp",
        hash: input_hash(text),
        units: fh_core_solver::NO_UNITS,
    };
    let options = Options {
        method,
        threads,
        formats: Formats::parse("vtu,vtk,text").unwrap(),
        ..Options::default()
    };
    let mut steps = Vec::new();
    let mut emit = |l: &str| {
        if l.starts_with("step ") {
            steps.push(l.to_string());
        }
    };
    run_model(&model, &input, &d, &options, false, &mut emit, &|| false).unwrap();
    (d, steps)
}

/// The result files and the restart files of `d`.
fn files(d: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v = snapshot(d);
    for name in ["restart.fh", "restart.fh.prev"] {
        if let Ok(bytes) = fs::read(d.join(name)) {
            v.push((name.to_string(), bytes));
        }
    }
    assert_eq!(v.last().map(|x| x.0.as_str()), Some("restart.fh.prev"));
    v
}

fn same_files_for_each_thread_count(text: &str, method: Method, name: &str, free: usize) {
    let (one, steps) = run(text, name, method, 1);
    assert!(!steps.is_empty());
    for l in &steps {
        assert!(
            l.contains(&format!("free_dofs={free} method={name} ")),
            "{l}"
        );
    }
    let want = files(&one);
    for threads in [2, 4, 0] {
        let (d, s) = run(text, name, method, threads);
        assert_eq!(s, steps, "{threads} threads");
        assert_same(&want, &files(&d));
    }
}

#[test]
fn conjugate_gradients_give_the_same_files_on_1_2_4_and_all_threads() {
    same_files_for_each_thread_count(&cube(), Method::Pcg, "pcg", 9450);
}

#[test]
fn cholesky_gives_the_same_files_on_1_2_4_and_all_threads() {
    same_files_for_each_thread_count(&bar(), Method::Cholesky, "cholesky", 2400);
}
