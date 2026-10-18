//! `fh model`: refusals, hex8 output, and the path STEP, mesh, `.inp`, solve. The last test
//! skips when gmsh is missing.

mod common;
use common::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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
    let mut args = vec!["model", p(msh), "--case", p(dir), "-o", p(&out)];
    args.extend(extra);
    fh(&args)
}

/// `fh model` on the tet10 block fixture with mesh units SI-mm.
fn model_mm(dir: &Path) -> Output {
    model(&block("block-tet10.msh"), dir, &["--mesh-units", "SI-mm"])
}

#[test]
fn a_mesh_without_a_unit_record_is_refused() {
    let dir = case("nounit", |t| t);
    let out = model(&block("block-tet10.msh"), &dir, &[]);
    fails(&out, 1, "unit.mesh.missing");
}

#[test]
fn a_mesh_in_other_units_than_the_case_is_refused() {
    let dir = case("mismatch", |t| t);
    let out = model(&block("block-tet10.msh"), &dir, &["--mesh-units", "SI-m"]);
    fails(&out, 1, "unit.mesh.mismatch");
    assert!(!dir.join("out.inp").exists());
}

#[test]
fn a_recorded_unit_wins_over_a_conflicting_option() {
    let dir = case("conflict", |t| t);
    let msh = dir.join("a.msh");
    std::fs::copy(block("block-tet10.msh"), &msh).unwrap();
    std::fs::write(dir.join("a.msh.units"), "SI-mm\n").unwrap();
    says(
        &model(&msh, &dir, &["--mesh-units", "SI-m"]),
        "unit.mesh.conflict",
    );
}

#[test]
fn a_case_without_a_model_table_is_refused() {
    let dir = case("nomodel", |t| {
        // Remove everything from `[model]` to the comment above `[ref]`.
        let (head, tail) = t.split_once("[model]").unwrap();
        format!("{head}# Strain{}", tail.split_once("# Strain").unwrap().1)
    });
    fails(&model_mm(&dir), 1, "model.missing");
}

#[test]
fn a_set_with_no_node_is_refused() {
    // The block is 4 long in x. No node lies at x = 100.
    let dir = case("emptyset", |t| t.replace("at = 100.0", "at = 100.5"));
    // The block is 4 long, so the TIP plane at x = 100 holds no node. The case loaded.
    fails(&model_mm(&dir), 1, "model.set.empty");
}

#[test]
fn a_boundary_with_an_unknown_set_is_refused() {
    let dir = case("badset", |t| t.replace("set = \"TIP\"", "set = \"NOPE\""));
    let message = "`NOPE` is not a `[[model.set]]` or `[[model.region]]` name";
    says(&model_mm(&dir), message);
}

#[test]
fn the_inp_for_a_tet10_block_parses_with_the_sets_of_the_case() {
    let dir = case("tet10", |t| t.replace("at = 100.0", "at = 4.0"));
    let out = model_mm(&dir);
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
    let out = model_mm(&dir);
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
    fails(&model_mm(&dir), 1, "mesh.region.missing");
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
    fails(
        &model(&msh, &dir, &["--mesh-units", "SI-mm"]),
        1,
        "mesh.volume.unnamed",
    );
}

#[test]
fn the_workflow_commands_run_before_the_mesh_file_of_the_case_exists() {
    // The case names bar.inp, which fh model makes at the end of the workflow.
    let dir = case("before", |t| t);
    std::fs::remove_file(dir.join("bar.inp")).unwrap();
    let info = fh(&["info", p(&dir.join("bar.step")), "--case", p(&dir)]);
    assert_eq!(info.status.code(), Some(0), "{}", stderr(&info));
    fails(&model_mm(&dir), 1, "model.set.empty");
}

/// `fh mesh DIR/bar.step --case DIR --size SIZE -o DIR/bar.msh --gmsh GMSH`.
fn mesh_bar(dir: &Path, size: &str, gmsh: &Path) -> Output {
    let (step, msh) = (dir.join("bar.step"), dir.join("bar.msh"));
    let (s, d, m, g) = (p(&step), p(dir), p(&msh), p(gmsh));
    fh(&["mesh", s, "--case", d, "--size", size, "-o", m, "--gmsh", g])
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
    let (msh, inp) = (dir.join("bar.msh"), dir.join("run.inp"));
    let mesh = mesh_bar(&dir, "8", &gmsh);
    assert_eq!(mesh.status.code(), Some(0), "{}", stderr(&mesh));
    let made = fh(&["model", p(&msh), "--case", p(&dir), "-o", p(&inp)]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    let results = dir.join("results");
    let run = Command::new(&solver)
        .args([p(&inp), p(&results), "--formats", "text,vtu"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = stdout(&run);
    assert!(run.status.success(), "{text}{}", stderr(&run));
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
    let out = mesh_bar(&dir, "10", &gmsh);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout(&out);
    for region in ["SUPPORT", "SYM_Y", "SYM_Z", "LOAD"] {
        let row = format!("surface\t{region}\t");
        assert!(report.contains(&row), "{region}: {report}");
    }
    assert!(report.contains("elset\tSOLID\t"), "{report}");
    // A region on a plane with no face of the part is refused.
    let toml = std::fs::read_to_string(dir.join("case.toml")).unwrap();
    let toml = toml.replace("at = 100.0", "at = 150.0");
    std::fs::write(dir.join("case.toml"), toml).unwrap();
    fails(&mesh_bar(&dir, "10", &gmsh), 1, "mesh.region.missing");
}
