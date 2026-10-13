//! The three result formats: plain text, VTU and legacy VTK.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use fh_core_solver::{run_model, Formats, Input, Options};
use fh_io::vtu::read_vtu;

fn fixture() -> String {
    fs::read_to_string(format!(
        "{}/../../fixtures/inp/cantilever.inp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fh-formats-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

fn run(name: &str, formats: Formats) -> PathBuf {
    let d = dir(name);
    let model = fh_inp::parse(&fixture()).unwrap();
    let options = Options {
        formats,
        ..Options::default()
    };
    let input = Input {
        name: "input.inp",
        hash: 1,
    };
    run_model(&model, &input, &d, &options, false, &mut |_| {}, &|| false).unwrap();
    d
}

#[test]
fn every_format_is_on_by_default() {
    let d = run("all", Formats::default());
    for f in ["step-1.txt", "step-1.vtu", "step-1.vtk", "results.pvd"] {
        assert!(d.join(f).is_file(), "{f} missing");
    }
}

#[test]
fn a_format_list_selects_the_files() {
    let f = Formats::parse("text,vtk").unwrap();
    assert!(f.text && f.vtk && !f.vtu);
    let d = run("some", f);
    assert!(d.join("step-1.txt").is_file());
    assert!(d.join("step-1.vtk").is_file());
    assert!(!d.join("step-1.vtu").exists());
    // The PVD lists the first grid format that is on.
    let pvd = fs::read_to_string(d.join("results.pvd")).unwrap();
    assert!(pvd.contains("step-1.vtk"), "{pvd}");
}

#[test]
fn a_bad_format_name_is_an_error() {
    assert!(Formats::parse("text,hdf5").unwrap_err().contains("hdf5"));
    assert!(Formats::parse("").is_err());
}

#[test]
fn the_text_file_has_a_header_and_one_row_for_each_entity() {
    let d = run("text", Formats::default());
    let t = fs::read_to_string(d.join("step-1.txt")).unwrap();
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[0], "fh-results 1");
    assert!(t.contains("\nunits: "), "{t}");
    assert!(t.contains("\nstep: 1\n"), "{t}");
    assert!(t.contains("\nstep-name: TIP-LOAD\n"), "{t}");
    assert!(t.contains("\ntime: 1\n"), "{t}");
    let section = |name: &str| -> Vec<&str> {
        let start = lines
            .iter()
            .position(|l| *l == name)
            .unwrap_or_else(|| panic!("{name}"));
        lines[start + 2..]
            .iter()
            .take_while(|l| !l.is_empty())
            .copied()
            .collect()
    };
    // 20 nodes, 4 elements of 8 integration points.
    assert_eq!(section("nodes: 20").len(), 20);
    assert_eq!(section("elements: 4").len(), 4);
    assert_eq!(section("integration-points: 32").len(), 32);
    assert!(t.contains("node_id U1 U2 U3 RF1 RF2 RF3"), "{t}");
    assert!(
        t.contains("element_id ip S11 S22 S33 S12 S23 S13 MISES"),
        "{t}"
    );
}

#[test]
fn text_and_vtk_hold_the_numbers_of_the_vtu() {
    let d = run("same", Formats::default());
    let grid = read_vtu(&fs::read(d.join("step-1.vtu")).unwrap()).unwrap();
    let u = grid.point_field("U").unwrap().as_f64().unwrap().to_vec();
    let s = grid.cell_field("S").unwrap().as_f64().unwrap().to_vec();

    // Text: the node rows are in the order of the model.
    let t = fs::read_to_string(d.join("step-1.txt")).unwrap();
    let lines: Vec<&str> = t.lines().collect();
    let at = lines.iter().position(|l| *l == "nodes: 20").unwrap() + 2;
    for (i, row) in lines[at..at + 20].iter().enumerate() {
        let v: Vec<f64> = row
            .split_whitespace()
            .skip(1)
            .map(|x| x.parse().unwrap())
            .collect();
        assert_eq!(v[..3], u[3 * i..3 * i + 3], "node row {i}");
    }
    let at = lines.iter().position(|l| *l == "elements: 4").unwrap() + 2;
    for (i, row) in lines[at..at + 4].iter().enumerate() {
        let v: Vec<f64> = row
            .split_whitespace()
            .skip(1)
            .map(|x| x.parse().unwrap())
            .collect();
        assert_eq!(v[..6], s[6 * i..6 * i + 6], "element row {i}");
    }

    // VTK: the displacement vectors.
    let k = fs::read_to_string(d.join("step-1.vtk")).unwrap();
    assert!(k.starts_with("# vtk DataFile Version 3.0\n"));
    assert!(k.contains("POINTS 20 double\n"));
    assert!(k.contains("CELLS 4 36\n"));
    let lines: Vec<&str> = k.lines().collect();
    let at = lines.iter().position(|l| *l == "VECTORS U double").unwrap() + 1;
    for i in 0..20 {
        let v: Vec<f64> = lines[at + i]
            .split(' ')
            .map(|x| x.parse().unwrap())
            .collect();
        assert_eq!(v, u[3 * i..3 * i + 3], "vtk node {i}");
    }
}

#[test]
fn the_mean_of_the_point_stresses_matches_the_element_value() {
    // For a cube of equal volume weights the mean of the 8 point values is the element stress.
    let d = run("mean", Formats::default());
    let t = fs::read_to_string(d.join("step-1.txt")).unwrap();
    let lines: Vec<&str> = t.lines().collect();
    let e = lines.iter().position(|l| *l == "elements: 4").unwrap() + 2;
    let p = lines
        .iter()
        .position(|l| *l == "integration-points: 32")
        .unwrap()
        + 2;
    for el in 0..4 {
        let avg: Vec<f64> = lines[e + el]
            .split_whitespace()
            .skip(1)
            .map(|x| x.parse().unwrap())
            .collect();
        let mut sum = [0.0; 6];
        for ip in 0..8 {
            let row: Vec<f64> = lines[p + 8 * el + ip]
                .split_whitespace()
                .skip(2)
                .map(|x| x.parse().unwrap())
                .collect();
            for c in 0..6 {
                sum[c] += row[c] / 8.0;
            }
        }
        for c in 0..6 {
            assert!(
                (sum[c] - avg[c]).abs() <= 1e-9 * avg[c].abs().max(1e-3),
                "el {el} c {c}: {} {}",
                sum[c],
                avg[c]
            );
        }
    }
}

#[test]
fn the_executable_takes_a_formats_option() {
    let d = dir("exe");
    let inp = d.with_extension("inp");
    fs::write(&inp, fixture()).unwrap();
    let status = |extra: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_fh-solve"));
        c.arg(&inp)
            .arg(&d)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::null());
        let mut child = c.spawn().unwrap();
        let _keep = child.stdin.take();
        child.wait().unwrap().code()
    };
    assert_eq!(status(&["--formats", "vtk"]), Some(0));
    assert!(d.join("step-1.vtk").is_file());
    assert!(!d.join("step-1.vtu").exists());
    assert!(!d.join("step-1.txt").exists());
    assert_eq!(status(&["--formats", "hdf5"]), Some(2));
}
