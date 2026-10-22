//! `fh report`: one HTML file with the text of the agent, the wasm viewer and the results of the
//! runs that the agent names.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

mod common;
use common::{p, scratch};

/// A state folder with the run `run-000001`, a folder with a fake viewer, and a text file.
struct Lab {
    dir: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir = scratch(name);
        let out = dir.join("state/runs/run-000001/out");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(out.parent().unwrap().join("in")).unwrap();
        fs::write(out.join("../source"), "/work/a320/job.inp").unwrap();
        fs::write(out.join("../in/input.inp"), "*NODE\n1, 0, 0, 0\n").unwrap();
        fs::write(
            out.join("results.pvd"),
            "<VTKFile type=\"Collection\"><Collection units=\"SI-mm\">\
             <DataSet timestep=\"1\" part=\"0\" file=\"step-1.vtu\"/></Collection></VTKFile>",
        )
        .unwrap();
        fs::write(out.join("step-1.vtu"), b"fake vtu").unwrap();
        fs::create_dir_all(dir.join("viewer")).unwrap();
        fs::write(dir.join("viewer/fh_viewer.js"), "export default 1;").unwrap();
        fs::write(dir.join("viewer/fh_viewer_bg.wasm"), b"fake wasm").unwrap();
        fs::write(
            dir.join("text.html"),
            "<h1>A320 barrel</h1><p>The <a data-run=\"run-000001\">first run</a>.</p>",
        )
        .unwrap();
        Lab { dir }
    }

    fn fh(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fh"))
            .arg("report")
            .args(args)
            .env("FH_STATE", self.dir.join("state"))
            .env("FH_VIEWER", self.dir.join("viewer"))
            .output()
            .expect("run fh")
    }

    fn out(&self) -> PathBuf {
        self.dir.join("report.html")
    }

    fn report(&self, extra: &[&str]) -> Output {
        let (text, out) = (self.dir.join("text.html"), self.out());
        let mut args = vec![p(&text), "-o", p(&out)];
        args.extend_from_slice(extra);
        self.fh(&args)
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The report is 1 file. It holds the text, the viewer and the files that the viewer fetches
/// for the run, each under the path that the daemon serves. It loads nothing from the network.
#[test]
fn the_report_holds_the_text_the_viewer_and_the_run() {
    let lab = Lab::new("report-holds");
    let out = lab.report(&["--run", "run-000001"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert!(html.contains("<h1>A320 barrel</h1>"), "text");
    assert!(html.contains("a320/job.inp"), "name of the run");
    // The base64 of the bytes of each file, under the daemon path of the file.
    for (path, base64) in [
        ("/viewer/fh_viewer.js", "ZXhwb3J0IGRlZmF1bHQgMTs="),
        ("/viewer/fh_viewer_bg.wasm", "ZmFrZSB3YXNt"),
        ("/api/v1/runs/run-000001/input", "Kk5PREUKMSwgMCwgMCwgMAo="),
        ("/api/v1/runs/run-000001/files/step-1.vtu", "ZmFrZSB2dHU="),
    ] {
        let element = format!("data-path=\"{path}\">{base64}</script>");
        assert!(html.contains(&element), "{path} in the report");
    }
    assert!(
        html.contains("data-path=\"/api/v1/runs/run-000001/files/results.pvd\">"),
        "results.pvd"
    );
    assert!(
        !html.contains("http://") && !html.contains("https://"),
        "network"
    );
    assert!(html.contains("default-src 'none'"), "policy");
}

/// A run with no results, and a name that is no run, give no file.
#[test]
fn a_run_without_results_gives_no_report() {
    let lab = Lab::new("report-refuses");
    fs::remove_file(lab.dir.join("state/runs/run-000001/out/results.pvd")).unwrap();
    let out = lab.report(&["--run", "run-000001"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("run-000001 has no results"),
        "{}",
        stderr(&out)
    );
    assert!(!lab.out().exists());
    let out = lab.report(&["--run", "run-000009"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!lab.out().exists());
    let out = lab.report(&[]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("usage: fh report"),
        "{}",
        stderr(&out)
    );
}

/// A folder with no viewer files gives no file, and the message tells how to build them.
#[test]
fn no_viewer_gives_no_report() {
    let lab = Lab::new("report-no-viewer");
    fs::remove_file(lab.dir.join("viewer/fh_viewer_bg.wasm")).unwrap();
    let out = lab.report(&["--run", "run-000001"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("scripts/build-viewer.sh"),
        "{}",
        stderr(&out)
    );
    assert!(!lab.out().exists());
}

/// A curve with a line and measured points.
const CURVE: &str =
    "# title: Surge front\n# y: Z [1]\n# points measured: Martin and Moyce (1952)\n\
                     T [1],model,measured\n0.5,1.0,\n1.0,1.5,\n1.19,,1.44\n";

/// Each `--curve` is an x-y plot after the text: the line, the points over it and the source.
#[test]
fn the_report_draws_each_curve() {
    let lab = Lab::new("report-curve");
    let curve = lab.dir.join("front.csv");
    fs::write(&curve, CURVE).unwrap();
    let out = lab.report(&["--run", "run-000001", "--curve", p(&curve)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    let figure = html
        .find("<figure class=\"curve\"><figcaption>Surge front")
        .expect("plot");
    assert!(
        html.find("<h1>A320 barrel</h1>").unwrap() < figure,
        "after the text"
    );
    assert!(html.contains("<polyline"), "line");
    assert!(
        html.contains("<title>measured: 1.19, 1.44</title>"),
        "point"
    );
    assert!(html.contains("measured: Martin and Moyce (1952)"), "source");
}

/// A report of curves only needs no run and no viewer.
#[test]
fn a_report_of_curves_needs_no_run() {
    let lab = Lab::new("report-curve-only");
    fs::remove_dir_all(lab.dir.join("viewer")).unwrap();
    let curve = lab.dir.join("front.csv");
    fs::write(&curve, CURVE).unwrap();
    let out = lab.report(&["--curve", p(&curve)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert!(html.contains("<figure class=\"curve\">"));
    assert!(!html.contains("data-path=\"/viewer/"), "no viewer");
}

/// A bad curve file gives no report, and the message names the file and the line.
#[test]
fn a_bad_curve_gives_no_report() {
    let lab = Lab::new("report-bad-curve");
    let curve = lab.dir.join("front.csv");
    fs::write(&curve, "# y: Z [1]\nT [1],model\n0.5,one\n").unwrap();
    let out = lab.report(&["--run", "run-000001", "--curve", p(&curve)]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("front.csv: line 3: `one` is not a finite number"),
        "{}",
        stderr(&out)
    );
    assert!(!lab.out().exists());
}
