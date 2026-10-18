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
fn thermal_material_groups_become_sections() {
    let dir = case("thermal", |t| {
        t.replace("[model.material]", "[[model.material]]")
            .replace("at = 100.0", "at = 4.0")
            .replace(
                "name = \"STEEL\"",
                "name = \"BLOCK\"\nexpansion = 1.2e-5\nbox = [0, 0, 0, 100, 20, 10]",
            )
            .replace("[model]", "[model]\ntemperature_change = 100.0")
    });
    let out = model_mm(&dir);
    assert!(out.status.success(), "{}", stderr(&out));
    let inp = std::fs::read_to_string(dir.join("out.inp")).unwrap();
    let parsed = fh_inp::parse_all(&inp).unwrap();
    assert_eq!(parsed.materials[0].expansion, Some(1.2e-5));
    assert!(inp.contains("*TEMPERATURE\nALL, 100.0"));
    assert!(inp.contains("ELSET=BLOCK, MATERIAL=BLOCK"));
}

#[test]
fn material_groups_must_cover_each_solid_once() {
    let dir = case("material-groups", |t| {
        t.replace("at = 100.0", "at = 4.0")
            .replace("[model.material]", "[[model.material]]")
            .replace(
                "name = \"STEEL\"",
                "name = \"MISSING\"\nbox = [0, 0, 0, 4, 1, 1]",
            )
    });
    fails(&model_mm(&dir), 1, "mesh.material.missing");
    let msh = dir.join("overlap.msh");
    let text = std::fs::read_to_string(block("block-tet10.msh")).unwrap();
    // The solid has two physical tags, so every element has two materials.
    let text = text
        .replace("3\n2 1", "4\n2 1")
        .replace("3 3 \"BLOCK\"", "3 3 \"BLOCK\"\n3 4 \"OTHER\"")
        .replace("1 3 6", "2 3 4 6");
    std::fs::write(&msh, text).unwrap();
    let toml = std::fs::read_to_string(dir.join("case.toml"))
        .unwrap()
        .replace("MISSING", "BLOCK");
    let toml = format!("{toml}\n[[model.material]]\nname = \"OTHER\"\nyoungs_modulus = 70000\npoissons_ratio = 0.0\nbox = [0, 0, 0, 4, 1, 1]\n");
    std::fs::write(dir.join("case.toml"), toml).unwrap();
    fails(
        &model(&msh, &dir, &["--mesh-units", "SI-mm"]),
        1,
        "mesh.material.coverage",
    );
}

#[test]
fn warpage_step_keeps_both_materials_and_the_shared_interface() {
    let Some(gmsh) = real_gmsh() else {
        return;
    };
    let dir = scratch("warpage-step");
    let source = root().join("validation/cases/warpage-strip");
    for file in ["case.toml", "derivation.md", "strip.step"] {
        std::fs::copy(source.join(file), dir.join(file)).unwrap();
    }
    let msh = dir.join("strip.msh");
    let out = fh(&[
        "mesh",
        p(&dir.join("strip.step")),
        "--case",
        p(&dir),
        "--size",
        "1",
        "--gmsh",
        p(&gmsh),
        "-o",
        p(&msh),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let mesh = fh_io::read_msh(&std::fs::read_to_string(&msh).unwrap()).unwrap();
    let low = &mesh.nset("LOW").unwrap().ids;
    let high = &mesh.nset("HIGH").unwrap().ids;
    let shared: Vec<_> = mesh
        .nodes
        .iter()
        .filter(|n| low.contains(&n.id) && high.contains(&n.id))
        .collect();
    assert!(!shared.is_empty());
    assert!(shared.iter().all(|n| (n.xyz[2] - 1.0).abs() < 1e-6));
    let out = model(&msh, &dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let parsed = fh_inp::parse_all(&std::fs::read_to_string(dir.join("out.inp")).unwrap()).unwrap();
    assert_eq!(parsed.materials.len(), 2);
    assert_eq!(parsed.nset("ORIGIN").unwrap().ids.len(), 1);
    assert_eq!(
        parsed.elset("LOW").unwrap().ids.len() + parsed.elset("HIGH").unwrap().ids.len(),
        parsed.elements.len()
    );
    assert!(parsed.materials.iter().all(|m| m.expansion.is_some()));
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

#[test]
fn step_to_results_matches_the_analytical_bar() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let Some(solver) = ccx_solver() else { return };
    let dir = case("e2e", |t| t);
    let (msh, inp) = (dir.join("bar.msh"), dir.join("run.inp"));
    let mesh = mesh_bar(&dir, "8", &gmsh);
    assert_eq!(mesh.status.code(), Some(0), "{}", stderr(&mesh));
    let made = fh(&["model", p(&msh), "--case", p(&dir), "-o", p(&inp)]);
    assert_eq!(made.status.code(), Some(0), "{}", stderr(&made));
    let results = dir.join("results");
    let run = Command::new(&solver)
        .args([p(&inp), p(&results)])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(run.status.success(), "{}{}", stdout(&run), stderr(&run));
    let bytes = std::fs::read(results.join("step-1.vtu")).unwrap();
    let grid = fh_io::vtu::read_vtu(&bytes).unwrap();
    let u = grid.point_field("U").unwrap().as_f64().unwrap();
    let max_u = u
        .chunks(3)
        .map(|v| v.iter().map(|x| x * x).sum::<f64>().sqrt())
        .fold(0.0, f64::max);
    let s = grid.cell_field("S").unwrap().as_f64().unwrap();
    // The largest displacement is at a tip corner: 0.1 mm along x, and the Poisson contraction
    // 0.3e-3 * 20 mm and 0.3e-3 * 10 mm. The axial stress is E * 1e-3 = 210 MPa everywhere.
    // The .frd file of ccx has 6 significant digits.
    let corner = (0.1f64.powi(2) + 0.006f64.powi(2) + 0.003f64.powi(2)).sqrt();
    assert!((max_u - corner).abs() < 1e-6, "{max_u}");
    for s_xx in s.chunks(6).map(|c| c[0]) {
        assert!((s_xx - 210.0).abs() < 1e-3, "{s_xx}");
    }
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
