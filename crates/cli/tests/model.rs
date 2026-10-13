//! `fh model`: refusals, hex8 output, and the path STEP, mesh, `.inp`, solve. The last test
//! skips when gmsh is missing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(name: &str) -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fh-model-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fh(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(args)
        .output()
        .expect("run fh")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The bar-tension case in a new folder, with `edit` applied to its `case.toml`.
fn case(name: &str, edit: impl Fn(String) -> String) -> PathBuf {
    let src = root().join("validation/cases/bar-tension");
    let dir = scratch(name);
    for file in ["bar.step", "bar.inp", "derivation.md"] {
        std::fs::copy(src.join(file), dir.join(file)).unwrap();
    }
    let toml = std::fs::read_to_string(src.join("case.toml")).unwrap();
    std::fs::write(dir.join("case.toml"), edit(toml)).unwrap();
    dir
}

fn block(file: &str) -> PathBuf {
    root().join("fixtures/msh").join(file)
}

fn model(msh: &Path, dir: &Path, extra: &[&str]) -> Output {
    let out = dir.join("out.inp");
    let mut args = vec![
        "model",
        msh.to_str().unwrap(),
        "--case",
        dir.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ];
    args.extend(extra);
    fh(&args)
}

#[test]
fn a_mesh_without_a_unit_record_is_refused() {
    let dir = case("nounit", |t| t);
    let out = model(&block("block-tet10.msh"), &dir, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("unit.mesh.missing"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_mesh_in_other_units_than_the_case_is_refused() {
    let dir = case("mismatch", |t| t);
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-m"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("unit.mesh.mismatch"),
        "{}",
        stderr(&out)
    );
    assert!(!dir.join("out.inp").exists());
}

#[test]
fn a_recorded_unit_wins_over_a_conflicting_option() {
    let dir = case("conflict", |t| t);
    let msh = dir.join("a.msh");
    std::fs::copy(block("block-tet10.msh"), &msh).unwrap();
    std::fs::write(dir.join("a.msh.units"), "SI-mm\n").unwrap();
    let out = model(&msh, &dir, &["--mesh-units", "SI-m"]);
    assert!(
        stderr(&out).contains("unit.mesh.conflict"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_case_without_a_model_table_is_refused() {
    let dir = case("nomodel", |t| {
        // Remove everything from `[model]` to the comment above `[ref]`.
        let (head, tail) = t.split_once("[model]").unwrap();
        format!("{head}# Strain{}", tail.split_once("# Strain").unwrap().1)
    });
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("model.missing"), "{}", stderr(&out));
}

#[test]
fn a_set_with_no_node_is_refused() {
    // The block is 4 long in x. No node lies at x = 100.
    let dir = case("emptyset", |t| t.replace("at = 100.0", "at = 100.5"));
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("model.set.empty"), "{}", stderr(&out));
}

#[test]
fn a_boundary_with_an_unknown_set_is_refused() {
    let dir = case("badset", |t| t.replace("set = \"TIP\"", "set = \"NOPE\""));
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert!(
        stderr(&out).contains("`NOPE` is not a `[[model.set]]` name"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn the_inp_for_a_tet10_block_parses_with_the_sets_of_the_case() {
    let dir = case("tet10", |t| t.replace("at = 100.0", "at = 4.0"));
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let inp = fh_inp::parse(&std::fs::read_to_string(dir.join("out.inp")).unwrap()).unwrap();
    assert!(inp.elements.iter().all(|e| e.element_type == "C3D10"));
    for name in ["FIXED", "TIP", "SYM_Y", "SYM_Z", "ALL"] {
        let ids = inp
            .nset(name)
            .map(|s| s.ids.len())
            .or(inp.elset(name).map(|s| s.ids.len()));
        assert!(ids.is_some_and(|n| n > 0), "set {name}");
    }
    assert_eq!(inp.steps[0].name.as_deref(), Some("LOAD"));
    assert_eq!(inp.steps[0].boundaries.len(), 4);
}

#[test]
fn an_extruded_hex_block_gives_c3d8i_elements() {
    let dir = case("hex", |t| t.replace("at = 100.0", "at = 4.0"));
    let out = model(&block("block-hex.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let inp = fh_inp::parse(&std::fs::read_to_string(dir.join("out.inp")).unwrap()).unwrap();
    assert!(inp.elements.iter().all(|e| e.element_type == "C3D8I"));
}

fn real_gmsh() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FH_GMSH").filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("gmsh"))
        .find(|p| p.is_file())
}

/// The number after `key=` in the last `step` line of the solver output.
fn value(text: &str, key: &str) -> f64 {
    let line = text.lines().rev().find(|l| l.starts_with("step ")).unwrap();
    let word = line
        .split_whitespace()
        .find_map(|w| w.strip_prefix(key))
        .unwrap();
    word.parse().unwrap()
}

#[test]
fn step_to_results_matches_the_analytical_bar() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let fh_exe = Path::new(env!("CARGO_BIN_EXE_fh"));
    let solver = fh_exe.with_file_name(format!("fh-solve{}", std::env::consts::EXE_SUFFIX));
    if !solver.is_file() {
        eprintln!("skipped: fh-solve is not built");
        return;
    }
    let dir = case("e2e", |t| t);
    let (step, msh, inp) = (
        dir.join("bar.step"),
        dir.join("bar.msh"),
        dir.join("run.inp"),
    );
    let d = dir.to_str().unwrap();
    let mesh = fh(&[
        "mesh",
        step.to_str().unwrap(),
        "--case",
        d,
        "--size",
        "8",
        "-o",
        msh.to_str().unwrap(),
        "--gmsh",
        gmsh.to_str().unwrap(),
    ]);
    assert_eq!(mesh.status.code(), Some(0), "{}", stderr(&mesh));
    let made = fh(&[
        "model",
        msh.to_str().unwrap(),
        "--case",
        d,
        "-o",
        inp.to_str().unwrap(),
    ]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    let results = dir.join("results");
    // The solver stops at the end of stdin, so keep stdin open until it exits.
    let mut child = Command::new(&solver)
        .args([
            inp.to_str().unwrap(),
            results.to_str().unwrap(),
            "--formats",
            "text,vtu",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let run = child.wait_with_output().unwrap();
    drop(stdin);
    let text = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&run.stderr)
    );
    // The largest displacement is at a tip corner: 0.1 mm along x, and the Poisson contraction
    // 0.3e-3 * 20 mm and 0.3e-3 * 10 mm. The von Mises stress is E * 1e-3 = 210 MPa.
    let corner = (0.1f64.powi(2) + 0.006f64.powi(2) + 0.003f64.powi(2)).sqrt();
    assert!((value(&text, "max_u=") - corner).abs() < 1e-9, "{text}");
    assert!((value(&text, "max_mises=") - 210.0).abs() < 1e-3, "{text}");
    assert!(results.join("results.pvd").is_file());
}
