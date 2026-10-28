//! `fh-quine CASE`: solve the grid cases of the validation case in the folder `CASE` with the
//! native solvers of femahoy, in this process and thread, and compare the results with the
//! references and the tolerance of `CASE/check.toml`. The results go to `results/` in the working
//! folder, and the table of the check to `results/check.txt`. Exit code 0 when the case passes.
//!
//! For wasm32-unknown-emscripten this crate is a static library with the C function `main`.
//! quine-wasm links it with emcc into 1 HTML file (`scripts/build-quine.sh`, `docs/reference/quine.md`).

use std::path::Path;
use std::time::Instant;

use fh::validation::{run_case_with, summary, Case, Native};

/// The folder of the results, in the working folder.
pub const RESULTS: &str = "results";

/// Run the command with the arguments after the program name. The return value is the exit code.
pub fn run(args: &[String]) -> i32 {
    let [dir] = args else {
        eprintln!("usage: fh-quine CASE");
        return 2;
    };
    let dir = Path::new(dir);
    let case = match Case::load(dir) {
        Ok(case) => case,
        Err(e) => {
            eprintln!("fh-quine: {e}");
            return 2;
        }
    };
    // The comment lines of check.toml tell the problem, the reference and the tolerance budget.
    let check = std::fs::read_to_string(dir.join("check.toml")).unwrap_or_default();
    for line in check.lines().take_while(|l| l.starts_with('#')) {
        println!("{line}");
    }
    println!("solving {} ...", case.id);
    let out = Path::new(RESULTS);
    let _ = std::fs::remove_dir_all(out);
    let start = Instant::now();
    let report = run_case_with(&case, &Native { run: out.into() });
    let (text, passed) = summary(&[(&case, report)]);
    print!("{text}");
    println!("time {:.1} s", start.elapsed().as_secs_f64());
    if let Err(e) =
        std::fs::create_dir_all(out).and_then(|()| std::fs::write(out.join("check.txt"), &text))
    {
        eprintln!("fh-quine: {}: {e}", out.join("check.txt").display());
    }
    if passed {
        0
    } else {
        1
    }
}

/// The C entry point. std reads no arguments for this target, so they come from `argv`.
///
/// # Safety
///
/// `argv` holds `argc` pointers to strings that end in 0, as the C runtime gives them.
#[cfg(target_os = "emscripten")]
#[no_mangle]
pub unsafe extern "C" fn main(
    argc: std::ffi::c_int,
    argv: *const *const std::ffi::c_char,
) -> std::ffi::c_int {
    use std::io::Write;
    let args: Vec<String> = (1..argc.max(0) as usize)
        .map(|i| unsafe { std::ffi::CStr::from_ptr(*argv.add(i)) })
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let code = run(&args);
    let _ = std::io::stdout().flush();
    code
}
