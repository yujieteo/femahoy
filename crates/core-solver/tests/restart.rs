//! Restart after a crash: atomic files, checksums, fall back to the older file, truncated
//! results, and runs that die at any point and resume to the same answer.

mod common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fh_core_solver::element::{small_strain, Material};
use fh_core_solver::restart::{read_latest, Restart};
use fh_core_solver::{
    input_hash, run_model, Formats, Input, MaterialFactory, Options, SolverError,
};

// ---- A material with state, for runs through the library ----

struct Bilinear {
    d: [[f64; 6]; 6],
}

impl Material for Bilinear {
    fn state_len(&self) -> usize {
        1
    }
    fn is_linear(&self) -> bool {
        false
    }
    fn update(
        &self,
        f: &[[f64; 3]; 3],
        old: &[f64],
        new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        const C: [f64; 6] = [1.0, 1.0, 1.0, 0.5, 0.5, 0.5];
        let eps = small_strain(f);
        let e = (0..6).map(|i| C[i] * eps[i] * eps[i]).sum::<f64>().sqrt();
        let (ey, k) = (0.01, 0.1);
        let kappa = old[0].max(e);
        new[0] = kappa;
        let (s, ds) = if kappa <= ey {
            (1.0, 0.0)
        } else {
            (
                (ey + k * (kappa - ey)) / kappa,
                (k * kappa - (ey + k * (kappa - ey))) / (kappa * kappa),
            )
        };
        let loading = e > old[0] && e > ey;
        let de: Vec<f64> = (0..6)
            .map(|i| (0..6).map(|j| self.d[i][j] * eps[j]).sum())
            .collect();
        let mut t = [[0.0; 6]; 6];
        for i in 0..6 {
            for j in 0..6 {
                t[i][j] = s * self.d[i][j];
                if loading {
                    t[i][j] += de[i] * ds * C[j] * eps[j] / e;
                }
            }
        }
        let mut stress = [0.0; 6];
        for i in 0..6 {
            stress[i] = s * de[i];
        }
        Ok((stress, t))
    }
}

fn options() -> Options {
    Options {
        materials: Some(MaterialFactory::new(|_| {
            Ok(Box::new(Bilinear {
                d: fh_core_solver::element::elasticity(1000.0, 0.0),
            }))
        })),
        ..Options::default()
    }
}

const BAR: &str = "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
*ELEMENT, TYPE=C3D8\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0\n*SOLID SECTION, ELSET=E, MATERIAL=M\n";

/// Two steps: up to strain 0.21, then down to 0.07. Five and two increments.
fn bar_input() -> String {
    let step = |inc: &str, load: f64| {
        format!(
            "*STEP\n*STATIC\n{inc}\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,2,3\n6,2,2\n\
*CLOAD\n2,1,{load}\n3,1,{load}\n6,1,{load}\n7,1,{load}\n*END STEP\n"
        )
    };
    format!(
        "{BAR}{}{}",
        step("0.2, 1., 0.01, 0.2", 7.5),
        step("0.5, 1.", 2.5)
    )
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fh-restart-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run(
    text: &str,
    run_dir: &Path,
    opts: &Options,
    restart: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<(), String> {
    let model = fh_inp::parse(text).map_err(|e| e.to_string())?;
    let input = Input {
        name: "input.inp",
        hash: input_hash(text),
    };
    run_model(&model, &input, run_dir, opts, restart, &mut |_| {}, cancel)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Every file of the folder except the restart files and the input. The wall time and the
/// run id (the folder name) of the summary differ from run to run, so they are left out.
fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            !n.starts_with("restart") && !n.ends_with(".inp")
        })
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let mut bytes = fs::read(e.path()).unwrap();
            if name == "summary.toon" {
                let text = String::from_utf8(bytes).unwrap();
                let kept: Vec<&str> = text
                    .lines()
                    .filter(|l| !l.contains("wall_s:") && !l.starts_with("run_id:"))
                    .collect();
                bytes = kept.join("\n").into_bytes();
            }
            (name, bytes)
        })
        .collect();
    v.sort();
    v
}

fn assert_same(a: &[(String, Vec<u8>)], b: &[(String, Vec<u8>)]) {
    let names = |v: &[(String, Vec<u8>)]| v.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    assert_eq!(names(a), names(b));
    for (x, y) in a.iter().zip(b) {
        if x.0.ends_with(".toon") || x.0.ends_with(".pvd") {
            assert_eq!(
                String::from_utf8_lossy(&x.1),
                String::from_utf8_lossy(&y.1),
                "{} differs",
                x.0
            );
        }
        assert!(x.1 == y.1, "{} differs", x.0);
    }
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
        "{}*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,2,3\n6,2,2\n*CLOAD\n2,1,1\n*END STEP\n",
        BAR
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
    let b = common::hex_block([8, 8, 8], [1.0, 1.0, 1.0], 0.1, "C3D8");
    let mut t = String::from("*NODE\n");
    for (i, p) in b.nodes.iter().enumerate() {
        t.push_str(&format!("{},{},{},{}\n", i + 1, p[0], p[1], p[2]));
    }
    t.push_str("*ELEMENT, TYPE=C3D8\n");
    for (i, e) in b.elements.iter().enumerate() {
        let ids: Vec<String> = e.iter().map(|n| (n + 1).to_string()).collect();
        t.push_str(&format!("{},{}\n", i + 1, ids.join(",")));
    }
    t.push_str(&format!(
        "*ELSET, ELSET=ALL, GENERATE\n1,{},1\n*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.25\n\
*SOLID SECTION, ELSET=ALL, MATERIAL=M\n",
        b.elements.len()
    ));
    let at = |axis: usize, v: f64| -> Vec<usize> {
        b.nodes
            .iter()
            .enumerate()
            .filter(|(_, p)| (p[axis] - v).abs() < 1e-12)
            .map(|(i, _)| i + 1)
            .collect()
    };
    for s in 0..8 {
        t.push_str("*STEP\n*STATIC\n*BOUNDARY\n");
        for n in at(0, 0.0) {
            t.push_str(&format!("{n},1,3\n"));
        }
        t.push_str("*CLOAD\n");
        for n in at(0, 1.0) {
            t.push_str(&format!("{n},1,{}\n", 0.1 * (s + 1) as f64));
        }
        t.push_str("*END STEP\n");
    }
    t
}

fn spawn(inp: &Path, d: &Path, restart: bool) -> std::process::Child {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fh-solve"));
    c.arg(inp).arg(d).args(["--formats", "vtu,text"]);
    if restart {
        c.arg("--restart");
    }
    // Hold stdin open: the end of stdin is a cancel.
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    c.env("FH_THREADS", "2").spawn().unwrap()
}

/// Read the event lines of `child`. `kill` gets each line and returns true to kill the
/// process at that line. Returns true when the kill happened, and false when the run ended.
fn run_until(mut child: std::process::Child, kill: &mut dyn FnMut(&str) -> bool) -> bool {
    let _hold = child.stdin.take();
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

#[test]
fn the_formats_option_keeps_working_with_restart() {
    let f = Formats::parse("vtk").unwrap();
    assert!(f.vtk && !f.vtu && !f.text);
}
