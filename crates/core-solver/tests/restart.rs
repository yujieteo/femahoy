//! Restart after a crash: atomic files, checksums, fall back to the older file, truncated
//! results, and runs that die at any point and resume to the same answer.

mod common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{assert_same, block_input, dir, pull, run, snapshot, Bilinear, CUBE, MAT};
use fh_core_solver::restart::{read_latest, Restart};
use fh_core_solver::{input_hash, Formats, MaterialFactory, Options};

fn options() -> Options {
    Options {
        materials: Some(MaterialFactory::new(|_| {
            Ok(Box::new(Bilinear::new(
                1000.0,
                0.0,
                0.01,
                0.1,
                f64::INFINITY,
            )))
        })),
        formats: Formats::parse("all").unwrap(),
        ..Options::default()
    }
}

/// Two steps: up to strain 0.21, then down to 0.07. Five and two increments.
fn bar_input() -> String {
    let (one, two) = (pull("0.2, 1., 0.01, 0.2", 7.5), pull("0.5, 1.", 2.5));
    format!("{CUBE}{MAT}{one}{two}")
}

// ---- Result files: one per increment, written once ----

#[test]
fn results_are_written_at_each_increment_end() {
    let d = dir("files");
    run(&bar_input(), &d, &options(), false, &|| false).unwrap();
    let names: Vec<String> = snapshot(&d).into_iter().map(|x| x.0).collect();
    for want in [
        "step-1-inc-1.vtu",
        "step-1-inc-4.vtk",
        "step-1-inc-4.txt",
        "step-1.vtu",
        "step-2.vtu",
        "results.pvd",
        "summary.toon",
    ] {
        assert!(names.contains(&want.to_string()), "{want} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
    let pvd = fs::read_to_string(d.join("results.pvd")).unwrap();
    assert!(
        pvd.contains("step-1-inc-1.vtu") && pvd.contains("step-2.vtu"),
        "{pvd}"
    );
}

// ---- The restart file ----

#[test]
fn the_restart_file_has_a_version_a_hash_and_a_checksum_and_the_last_two_are_kept() {
    let d = dir("format");
    let text = bar_input();
    run(&text, &d, &options(), false, &|| false).unwrap();
    let bytes = fs::read(d.join("restart.fh")).unwrap();
    assert_eq!(&bytes[..4], b"FHRS");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 3);
    let r = Restart::decode(&bytes).unwrap();
    assert_eq!(r.hash, input_hash(&text));
    assert_eq!((r.step, r.complete), (1, true));
    assert_eq!(r.displacement.len(), 24);
    assert!((r.lambda - 1.0).abs() < 1e-15);
    assert!(r.next_size > 0.0);
    assert!(
        !r.image.ip.is_empty(),
        "a nonlinear run keeps the integration point state"
    );
    let prev = fs::read(d.join("restart.fh.prev")).unwrap();
    let p = Restart::decode(&prev).unwrap();
    assert!(p.step < r.step || (p.step == r.step && p.number < r.number));
    let files: Vec<String> = fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("restart"))
        .collect();
    assert_eq!(files.len(), 2, "{files:?}");
}

#[test]
fn one_flipped_bit_fails_the_checksum() {
    let d = dir("checksum");
    run(&bar_input(), &d, &options(), false, &|| false).unwrap();
    let mut bytes = fs::read(d.join("restart.fh")).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    assert!(Restart::decode(&bytes).unwrap_err().contains("checksum"));
    let short = &bytes[..bytes.len() - 5];
    assert!(Restart::decode(short).is_err());
}

#[test]
fn a_linear_restart_file_holds_only_the_displacement() {
    let d = dir("linear");
    let text = format!(
        "{CUBE}{MAT}*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,2,3\n6,2,2\n*CLOAD\n2,1,1\n*END STEP\n"
    );
    run(&text, &d, &Options::default(), false, &|| false).unwrap();
    let bytes = fs::read(d.join("restart.fh")).unwrap();
    let r = Restart::decode(&bytes).unwrap();
    assert!(r.image.ip.is_empty() && r.image.internal.is_empty() && r.contact.is_empty());
    assert!(bytes.len() < 8 * 24 + 1024, "{} bytes", bytes.len());
}

#[test]
fn the_restart_interval_counts_increments() {
    let d = dir("interval");
    let text = bar_input();
    let mut o = options();
    o.restart_every = 2;
    // Stop after increment 3 of step 1: the file written last is the one of increment 2.
    let dd = d.clone();
    run(&text, &d, &o, false, &move || {
        dd.join("step-1-inc-3.vtu").exists()
    })
    .unwrap_err();
    let r = Restart::decode(&fs::read(d.join("restart.fh")).unwrap()).unwrap();
    assert_eq!((r.step, r.number), (0, 2));
}

// ---- Resume ----

#[test]
fn a_changed_input_file_is_rejected() {
    let d = dir("changed");
    let text = bar_input();
    run(&text, &d, &options(), false, &|| false).unwrap();
    let changed = text.replace("1000, 0", "1001, 0");
    let e = run(&changed, &d, &options(), true, &|| false).unwrap_err();
    assert!(e.contains("not the one of the earlier run"), "{e}");
}

#[test]
fn a_restart_refuses_a_damaged_or_missing_result_of_a_committed_increment() {
    // The file, and true to remove it, false to damage it.
    for (file, remove) in [
        ("step-1-inc-1.vtu", false),
        ("step-1.vtu", true),
        ("step-1-inc-4.txt", true),
    ] {
        let d = dir("damaged");
        run(&bar_input(), &d, &options(), false, &|| false).unwrap();
        if remove {
            fs::remove_file(d.join(file)).unwrap();
        } else {
            fs::write(d.join(file), "broken").unwrap();
        }
        match run(&bar_input(), &d, &options(), true, &|| false) {
            Ok(()) => panic!("{file}: the restart passed"),
            Err(e) => assert!(e.contains(file), "{file}: {e}"),
        }
    }
}

#[test]
fn a_restart_without_a_restart_file_is_an_error() {
    let d = dir("none");
    assert!(run(&bar_input(), &d, &options(), true, &|| false).is_err());
}

/// Stop the run after the file `name` exists, then resume it.
fn stop_after(text: &str, d: &Path, file: &str) {
    let dd = d.to_path_buf();
    let f = file.to_string();
    let err = run(text, d, &options(), false, &move || dd.join(&f).exists()).unwrap_err();
    assert!(err.contains("cancel"), "{err}");
}

#[test]
fn a_run_stopped_after_increment_k_resumes_to_the_same_result() {
    let text = bar_input();
    let whole = dir("whole");
    run(&text, &whole, &options(), false, &|| false).unwrap();
    let want = snapshot(&whole);
    for file in [
        "step-1-inc-1.vtu",
        "step-1-inc-2.vtu",
        "step-1-inc-4.vtu",
        "step-1.vtu",
        "step-2-inc-1.vtu",
    ] {
        let d = dir(&format!("k-{file}"));
        stop_after(&text, &d, file);
        run(&text, &d, &options(), true, &|| false).unwrap();
        assert_same(&want, &snapshot(&d));
    }
}

#[test]
fn a_bad_newest_restart_file_falls_back_to_the_older_one() {
    let text = bar_input();
    let whole = dir("fb-whole");
    run(&text, &whole, &options(), false, &|| false).unwrap();
    let d = dir("fallback");
    stop_after(&text, &d, "step-1-inc-3.vtu");
    let mut bytes = fs::read(d.join("restart.fh")).unwrap();
    let n = bytes.len();
    bytes[n / 2] ^= 0xff;
    fs::write(d.join("restart.fh"), bytes).unwrap();
    run(&text, &d, &options(), true, &|| false).unwrap();
    assert_same(&snapshot(&whole), &snapshot(&d));
}

#[test]
fn a_run_killed_during_a_restart_write_leaves_a_usable_folder() {
    let text = bar_input();
    let whole = dir("kw-whole");
    run(&text, &whole, &options(), false, &|| false).unwrap();
    let d = dir("killwrite");
    stop_after(&text, &d, "step-1-inc-2.vtu");
    // A half-written temporary file, a missing newest file, and a half-written result.
    let newest = fs::read(d.join("restart.fh")).unwrap();
    fs::write(d.join("restart.fh.tmp"), &newest[..newest.len() / 3]).unwrap();
    fs::remove_file(d.join("restart.fh")).unwrap();
    fs::write(d.join("step-1-inc-3.vtu.tmp"), b"half").unwrap();
    run(&text, &d, &options(), true, &|| false).unwrap();
    assert_same(&snapshot(&whole), &snapshot(&d));
}

#[test]
fn results_beyond_the_resumed_increment_are_deleted() {
    let text = bar_input();
    let whole = dir("tr-whole");
    run(&text, &whole, &options(), false, &|| false).unwrap();
    let d = dir("truncate");
    stop_after(&text, &d, "step-1-inc-2.vtu");
    // The run died after it wrote the results of increment 3 but before its restart file.
    fs::write(d.join("step-1-inc-3.vtu"), b"stale").unwrap();
    fs::write(d.join("step-1-inc-3.txt"), b"stale").unwrap();
    fs::write(d.join("step-2.vtu"), b"stale").unwrap();
    run(&text, &d, &options(), true, &|| false).unwrap();
    assert_same(&snapshot(&whole), &snapshot(&d));
}

// ---- Kill the real executable ----

/// A linear model with many steps, so that the run has many restart points.
fn long_input() -> String {
    block_input([8, 8, 8], [1.0, 1.0, 1.0], 8)
}

fn spawn(inp: &Path, d: &Path, restart: bool) -> std::process::Child {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fh-solve"));
    c.arg(inp).arg(d).args(["--formats", "vtu,vtk,text"]);
    if restart {
        c.arg("--restart");
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    c.env("FH_THREADS", "2").spawn().unwrap()
}

/// Read the event lines of `child`. `kill` gets each line and returns true to kill the
/// process at that line. Returns true when the kill happened, and false when the run ended.
fn run_until(mut child: std::process::Child, kill: &mut dyn FnMut(&str) -> bool) -> bool {
    let out = BufReader::new(child.stdout.take().unwrap());
    for line in out.lines() {
        let Ok(line) = line else { break };
        if kill(&line) {
            child.kill().unwrap();
            child.wait().unwrap();
            return true;
        }
    }
    assert!(child.wait().unwrap().success(), "the run failed");
    false
}

#[test]
fn killing_fh_solve_three_times_and_resuming_gives_the_same_result() {
    let base = dir("kill");
    let inp = base.join("long.inp");
    let text = long_input();
    fs::write(&inp, &text).unwrap();
    let whole = base.join("whole");
    let mut points = 0;
    run_until(spawn(&inp, &whole, false), &mut |l| {
        points += usize::from(l.starts_with("restart "));
        false
    });
    let want = snapshot(&whole);
    assert_eq!(points, 8, "one restart point for each step");

    // Three random restart points a1 <= a2 <= a3 below the last two, and a kill a few event
    // lines after each one. Each run starts at the newest restart file, so it reaches its
    // point, and the kill comes before the run ends.
    let d = base.join("killed");
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as u64
        | 1;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let mut targets: Vec<(usize, usize)> = (0..3)
        .map(|_| (next(points as u64 - 1) as usize, 1 + next(3) as usize))
        .collect();
    targets.sort();
    let hash = input_hash(&text);
    let mut restart = false;
    for &(point, after) in &targets {
        // The restart point that this run starts from.
        let mut at = read_latest(&d, hash).map_or(0, |r| r.step + 1);
        let mut extra = 0;
        let killed = run_until(spawn(&inp, &d, restart), &mut |l| {
            if at >= point {
                extra += 1;
            }
            at += usize::from(l.starts_with("restart "));
            extra > after
        });
        assert!(killed, "targets {targets:?}: the run ended before the kill");
        restart = read_latest(&d, hash).is_ok();
    }
    run_until(spawn(&inp, &d, restart), &mut |_| false);
    eprintln!("kills (restart point, event lines after it): {targets:?}");
    assert_same(&want, &snapshot(&d));
}

// ---- SIGKILL in the middle of a write and in the middle of an increment ----
//
// `fh` has no run command in M2, so these tests kill `fh-solve`, the solver process that
// `fh` starts. `Child::kill` sends SIGKILL on Unix: the process gets no chance to clean up.

/// A bar of 48 x 6 x 6 C3D8 elements with 4 steps. A run of the debug build takes some
/// seconds, and most of it is the Cholesky factor. Each start of the process factors again.
fn bar_of_4_steps() -> String {
    block_input([48, 6, 6], [8.0, 1.0, 1.0], 4)
}

/// Start the bar, and return the input file, the folder of an uninterrupted run and its
/// result files.
fn uninterrupted(name: &str) -> (PathBuf, PathBuf, Vec<(String, Vec<u8>)>) {
    let base = dir(name);
    let inp = base.join("bar.inp");
    fs::write(&inp, bar_of_4_steps()).unwrap();
    let whole = base.join("whole");
    run_until(spawn(&inp, &whole, false), &mut |_| false);
    let want = snapshot(&whole);
    (inp, base, want)
}

/// The newest usable restart point of `d` as (step index, increment, step complete).
fn latest(d: &Path, hash: u64) -> Option<(usize, usize, bool)> {
    read_latest(d, hash)
        .ok()
        .map(|r| (r.step, r.number, r.complete))
}

/// The test feature pauses the process after it opens this temporary file.
/// SIGKILL then comes before the rename, regardless of the speed of the solver.
fn kill_at_file(inp: &Path, d: &Path, restart: bool, file: &str) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fh-solve"));
    c.arg(inp).arg(d).args(["--formats", "vtu,vtk,text"]);
    if restart {
        c.arg("--restart");
    }
    let child = c
        .env("FH_THREADS", "2")
        .env("FH_TEST_PAUSE_WRITE", file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let killed = run_until(child, &mut |line| line == format!("test-write {file}"));
    assert!(killed, "the run ended before the write of {file}");
    assert!(d.join(file).exists(), "SIGKILL must leave {file}");
}

#[test]
fn sigkill_in_the_middle_of_three_writes_and_a_resume_give_the_same_bytes() {
    let (inp, base, want) = uninterrupted("sigkill-write");
    let hash = input_hash(&bar_of_4_steps());
    let d = base.join("killed");
    // Stop at file creation, before any rename. No scheduling delay can miss the write.
    let mut restart = false;
    let mut moments = Vec::new();
    for (file, point) in [
        ("step-2.vtu.tmp", (0, 1, true)),
        ("restart.fh.tmp", (0, 1, true)),
        ("step-4.txt.tmp", (2, 1, true)),
    ] {
        let before = latest(&d, hash);
        kill_at_file(&inp, &d, restart, file);
        let after = latest(&d, hash);
        assert_eq!(after, Some(point), "kill at {file}");
        restart = after.is_some();
        moments.push((file, before, after));
    }
    eprintln!("kills (file, restart point before, after): {moments:?}");
    run_until(spawn(&inp, &d, restart), &mut |_| false);
    assert_same(&want, &snapshot(&d));
}

/// The fraction of a `progress` line of `step` (for example "2/4") in `phase`.
fn fraction(line: &str, step: &str, phase: &str) -> Option<f64> {
    line.strip_prefix(&format!("progress step={step} phase={phase} fraction="))?
        .parse()
        .ok()
}

#[test]
fn sigkill_in_the_middle_of_three_increments_and_a_resume_give_the_same_bytes() {
    let (inp, base, want) = uninterrupted("sigkill-increment");
    let hash = input_hash(&bar_of_4_steps());
    let d = base.join("killed");
    // Three moments in step 2: the stress recovery (a new run, which reuses the factor of
    // step 1), the assembly (a resumed run assembles again), and the Cholesky factor (the
    // first solve of a resumed run).
    type At = fn(&str) -> bool;
    let moments: [(&str, At); 3] = [
        ("recovery", |l| {
            fraction(l, "2/4", "recover").is_some_and(|f| f >= 0.25)
        }),
        ("assembly", |l| {
            fraction(l, "2/4", "assemble").is_some_and(|f| f >= 0.25)
        }),
        ("factor", |l| fraction(l, "2/4", "solve") == Some(0.0)),
    ];
    let mut restart = false;
    for (what, at) in moments {
        let killed = run_until(spawn(&inp, &d, restart), &mut |l| at(l));
        assert!(killed, "the run ended before the kill in the {what}");
        // The kill came before the end of the increment: the restart point is still the
        // end of step 1, and no result file of step 2 exists.
        assert_eq!(latest(&d, hash), Some((0, 1, true)), "kill in the {what}");
        let names: Vec<String> = fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("step-2"))
            .collect();
        assert!(names.is_empty(), "kill in the {what}: {names:?}");
        restart = true;
    }
    run_until(spawn(&inp, &d, restart), &mut |_| false);
    assert_same(&want, &snapshot(&d));
}

#[test]
fn the_formats_option_keeps_working_with_restart() {
    let f = Formats::parse("vtk").unwrap();
    assert!(f.vtk && !f.vtu && !f.text);
}
