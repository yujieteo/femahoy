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
    // The block is 4 long, so the TIP plane at x = 100 holds no node. The case loaded.
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("model.set.empty"), "{}", stderr(&out));
}

#[test]
fn a_boundary_with_an_unknown_set_is_refused() {
    let dir = case("badset", |t| t.replace("set = \"TIP\"", "set = \"NOPE\""));
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert!(
        stderr(&out).contains("`NOPE` is not a `[[model.set]]` or `[[model.region]]` name"),
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

/// The bar-tension case with the region TIP (the face x = 4 of the block fixture, a gmsh
/// physical surface) and a pull on it. The plane set TIP of the case becomes END, because a
/// region name must not repeat a set name.
fn region_case(name: &str, region: &str) -> PathBuf {
    case(name, |t| {
        t.replace("at = 100.0", "at = 4.0")
            .replace("name = \"TIP\"", "name = \"END\"")
            .replace("set = \"TIP\"", "set = \"END\"")
            .replace(
                "[[model.boundary]]\nset = \"FIXED\"",
                &format!(
                    "[[model.region]]\nname = \"{region}\"\naxis = \"x\"\nat = 4.0\n\n\
[[model.dsload]]\nsurface = \"{region}\"\npressure = -10.0\n\n\
[[model.boundary]]\nset = \"FIXED\""
                ),
            )
    })
}

#[test]
fn a_region_becomes_a_node_set_and_a_surface_with_a_pressure() {
    let dir = region_case("region", "TIP");
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = std::fs::read_to_string(dir.join("out.inp")).unwrap();
    assert_eq!(text.lines().nth(1), Some("** units: SI-mm"));
    let inp = fh_inp::parse(&text).unwrap();
    let faces = &inp.element_surface("TIP").unwrap().faces;
    let tip = &inp.nset("TIP").unwrap().ids;
    assert!(!faces.is_empty() && !tip.is_empty());
    // Every corner of every face lies on x = 4.
    let x: std::collections::HashMap<u32, f64> =
        inp.nodes.iter().map(|n| (n.id, n.xyz[0])).collect();
    for &(e, f) in faces {
        let el = inp.elements.iter().find(|el| el.id == e).unwrap();
        let corners = [[0, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]][usize::from(f) - 1];
        for c in corners {
            assert!((x[&el.nodes[c]] - 4.0).abs() < 1e-9, "element {e} face {f}");
        }
    }
    let d = &inp.steps[0].dsloads;
    assert_eq!((d[0].surface.as_str(), d[0].pressure), ("TIP", -10.0));
}

#[test]
fn a_lost_or_renamed_region_is_refused() {
    // The block fixture has the surface groups FIX and TIP, not PULL.
    let dir = region_case("lost", "PULL");
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("mesh.region.missing"),
        "{}",
        stderr(&out)
    );
    assert!(!dir.join("out.inp").exists());
}

#[test]
fn a_volume_with_no_physical_group_is_refused() {
    let dir = case("unnamed", |t| t.replace("at = 100.0", "at = 4.0"));
    let text = std::fs::read_to_string(block("block-tet10.msh")).unwrap();
    // The volume entity line ends with: 1 physical tag (3), 6 surfaces. Drop the tag.
    let unnamed = text.replace(" 1 3 6 -1 26 13 17 21 25", " 0 6 -1 26 13 17 21 25");
    assert_ne!(text, unnamed);
    let msh = dir.join("unnamed.msh");
    std::fs::write(&msh, unnamed).unwrap();
    let out = model(&msh, &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("mesh.volume.unnamed"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn the_workflow_commands_run_before_the_mesh_file_of_the_case_exists() {
    // The case names bar.inp, which fh model makes at the end of the workflow.
    let dir = case("before", |t| t);
    std::fs::remove_file(dir.join("bar.inp")).unwrap();
    let step = dir.join("bar.step");
    let info = fh(&[
        "info",
        step.to_str().unwrap(),
        "--case",
        dir.to_str().unwrap(),
    ]);
    assert_eq!(info.status.code(), Some(0), "{}", stderr(&info));
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-mm"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("model.set.empty"), "{}", stderr(&out));
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

#[test]
fn fh_mesh_makes_a_physical_group_for_each_region() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let src = root().join("validation/cases/bar-pressure");
    let dir = scratch("regions");
    for file in ["bar.step", "case.toml", "derivation.md"] {
        std::fs::copy(src.join(file), dir.join(file)).unwrap();
    }
    let (step, msh) = (dir.join("bar.step"), dir.join("bar.msh"));
    let args = |d: &Path| {
        [
            "mesh",
            step.to_str().unwrap(),
            "--case",
            d.to_str().unwrap(),
            "--size",
            "10",
            "-o",
            msh.to_str().unwrap(),
            "--gmsh",
            gmsh.to_str().unwrap(),
        ]
        .map(String::from)
    };
    let out = fh(&args(&dir).each_ref().map(String::as_str));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = String::from_utf8_lossy(&out.stdout);
    for region in ["SUPPORT", "SYM_Y", "SYM_Z", "LOAD"] {
        assert!(
            report.contains(&format!("surface\t{region}\t")),
            "{region}: {report}"
        );
    }
    assert!(report.contains("elset\tSOLID\t"), "{report}");
    // A region on a plane with no face of the part is refused.
    let toml = std::fs::read_to_string(dir.join("case.toml")).unwrap();
    std::fs::write(
        dir.join("case.toml"),
        toml.replace("at = 100.0", "at = 150.0"),
    )
    .unwrap();
    let out = fh(&args(&dir).each_ref().map(String::as_str));
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("mesh.region.missing"),
        "{}",
        stderr(&out)
    );
}
