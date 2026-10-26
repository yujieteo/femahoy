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

/// The bytes of the file under the daemon path `path` in the report `html`: raw deflate in base64.
fn file(html: &str, path: &str) -> Option<Vec<u8>> {
    let start = html.find(&format!("data-path=\"{path}\">"))? + path.len() + 13;
    let text = &html[start..start + html[start..].find('<')?];
    let digit = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let mut bytes = Vec::new();
    for chunk in text.trim_end_matches('=').as_bytes().chunks(4) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, c)| n | u32::from(digit(*c)) << (18 - 6 * i));
        bytes.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    Some(miniz_oxide::inflate::decompress_to_vec(&bytes).expect("raw deflate"))
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
    // The bytes of each file, under the daemon path of the file.
    for (path, bytes) in [
        ("/viewer/fh_viewer.js", &b"export default 1;"[..]),
        ("/viewer/fh_viewer_bg.wasm", b"fake wasm"),
        ("/api/v1/runs/run-000001/input", b"*NODE\n1, 0, 0, 0\n"),
        ("/api/v1/runs/run-000001/files/step-1.vtu", b"fake vtu"),
    ] {
        assert_eq!(
            file(&html, path).as_deref(),
            Some(bytes),
            "{path} in the report"
        );
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
    let out = lab.fh(&[p(&lab.dir.join("text.html"))]);
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
/// A plot goes in place of its mark in the text.
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
    fs::write(
        lab.dir.join("text.html"),
        "<h1>A320 barrel</h1><!--curve front.csv--><p>After the plot.</p>",
    )
    .unwrap();
    let out = lab.report(&["--curve", p(&curve)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    let figure = html.find("<figure class=\"curve\">").expect("plot");
    assert!(
        figure < html.find("After the plot.").unwrap(),
        "in place of the mark"
    );
    assert!(!html.contains("<!--curve"), "no mark");
}

/// A report of curves only needs no run and no viewer.
#[test]
fn a_report_of_curves_needs_no_run() {
    let lab = Lab::new("report-curve-only");
    fs::write(lab.dir.join("text.html"), "<h1>A320 barrel</h1>").unwrap();
    fs::remove_dir_all(lab.dir.join("viewer")).unwrap();
    let curve = lab.dir.join("front.csv");
    fs::write(&curve, CURVE).unwrap();
    let out = lab.report(&["--curve", p(&curve)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert!(html.contains("<figure class=\"curve\">"));
    assert!(!html.contains("data-path=\"/viewer/"), "no viewer");
}

/// A report with no run and no curve is the text only, with no viewer.
#[test]
fn a_report_of_the_text_only_needs_no_run_and_no_curve() {
    let lab = Lab::new("report-text-only");
    fs::write(lab.dir.join("text.html"), "<h1>A320 barrel</h1>").unwrap();
    fs::remove_dir_all(lab.dir.join("viewer")).unwrap();
    let out = lab.report(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert!(html.contains("<h1>A320 barrel</h1>"), "text");
    assert!(!html.contains("data-path="), "no files");
}

/// With `--one-step`, the report holds only the last step of the run, and its `results.pvd`
/// names only that step. When the text opens the run at some steps, the report holds those
/// steps, and each step number names its step in the kept steps. A slider keeps each of its
/// steps.
#[test]
fn the_one_step_option_keeps_only_the_steps_that_the_text_opens() {
    let lab = Lab::new("report-one-step");
    let out = lab.dir.join("state/runs/run-000001/out");
    fs::write(
        out.join("results.pvd"),
        "<VTKFile type=\"Collection\"><Collection units=\"SI-mm\">\
         <DataSet timestep=\"1\" part=\"0\" file=\"step-1.vtu\"/>\
         <DataSet timestep=\"2\" part=\"0\" file=\"step-2.vtu\"/>\
         <DataSet timestep=\"3\" part=\"0\" file=\"step-3.vtu\"/></Collection></VTKFile>",
    )
    .unwrap();
    fs::write(out.join("step-2.vtu"), b"middle vtu").unwrap();
    fs::write(out.join("step-3.vtu"), b"last vtu").unwrap();
    let out = lab.report(&["--run", "run-000001", "--one-step"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    let base = "/api/v1/runs/run-000001/files";
    assert_eq!(file(&html, &format!("{base}/step-1.vtu")), None);
    assert_eq!(
        file(&html, &format!("{base}/step-3.vtu")).as_deref(),
        Some(&b"last vtu"[..])
    );
    let pvd = String::from_utf8(file(&html, &format!("{base}/results.pvd")).unwrap()).unwrap();
    assert_eq!(
        pvd,
        "<VTKFile type=\"Collection\"><Collection units=\"SI-mm\">\
         <DataSet timestep=\"3\" part=\"0\" file=\"step-3.vtu\"/></Collection></VTKFile>"
    );
    fs::write(
        lab.dir.join("text.html"),
        "<button data-run=\"run-000001\" data-step=\"3\">end</button>\
         <button data-run=\"run-000001\" data-step=\"1\">start</button>",
    )
    .unwrap();
    let out = lab.report(&["--run", "run-000001", "--one-step"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert_eq!(file(&html, &format!("{base}/step-2.vtu")), None);
    assert!(file(&html, &format!("{base}/step-1.vtu")).is_some());
    assert!(file(&html, &format!("{base}/step-3.vtu")).is_some());
    assert!(
        html.contains("data-step=\"2\">end</button>")
            && html.contains("data-step=\"1\">start</button>"),
        "each control opens its step in the kept steps"
    );
    fs::write(
        lab.dir.join("text.html"),
        "<input type=\"range\" min=\"2\" max=\"3\" value=\"3\" data-run=\"run-000001\" \
         data-marks=\"3: 0,0,0 tip\">",
    )
    .unwrap();
    let out = lab.report(&["--run", "run-000001", "--one-step"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert_eq!(file(&html, &format!("{base}/step-1.vtu")), None);
    assert!(file(&html, &format!("{base}/step-2.vtu")).is_some());
    assert!(
        html.contains("min=\"1\" max=\"2\" value=\"2\"")
            && html.contains("data-marks=\"2: 0,0,0 tip\""),
        "the slider and the mark name their steps in the kept steps"
    );
}

/// A control of the text that names a run, a field or a step that the report does not have
/// gives no report, and the message names it.
#[test]
fn a_control_that_does_not_fit_its_run_gives_no_report() {
    let lab = Lab::new("report-check");
    let fixture = common::root().join("fixtures/results/cantilever-2step");
    let out = lab.dir.join("state/runs/run-000001/out");
    for name in ["results.pvd", "step-1.vtu", "step-2.vtu"] {
        fs::copy(fixture.join(name), out.join(name)).unwrap();
    }
    for (control, error) in [
        ("data-run=\"run-000009\"", "data-run=\"run-000009\""),
        ("data-run=\"run-000001\" data-step=\"3\"", "step \"3\""),
        (
            "data-run=\"run-000001\" data-marks=\"3: 0,0,0 tip\"",
            "step \"3\"",
        ),
        (
            "data-run=\"run-000001\" type=\"range\" min=\"1\"",
            "slider needs min and max",
        ),
        (
            "data-run=\"run-000001\" data-fields=\"MISES, Ez\"",
            "no field \"Ez\"",
        ),
    ] {
        fs::write(lab.dir.join("text.html"), format!("<input {control}>")).unwrap();
        let out = lab.report(&["--run", "run-000001"]);
        assert_eq!(out.status.code(), Some(1), "{control}");
        assert!(stderr(&out).contains(error), "{}", stderr(&out));
        assert!(!lab.out().exists());
    }
    fs::write(
        lab.dir.join("text.html"),
        "<a data-run=\"run-000001\" data-fields=\"MISES\" data-step=\"2\">run</a>",
    )
    .unwrap();
    let out = lab.report(&["--run", "run-000001"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
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

/// With `--viewer`, the report loads the shared viewer script beside it, by the nonce of its
/// script. It holds the files of the run and no copy of the viewer, so it needs no built viewer.
#[test]
fn a_report_with_a_shared_viewer_holds_no_viewer() {
    let lab = Lab::new("report-shared-viewer");
    fs::remove_dir_all(lab.dir.join("viewer")).unwrap();
    let out = lab.report(&["--run", "run-000001", "--viewer", "viewer.js"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert!(!html.contains("data-path=\"/viewer/"), "no viewer");
    assert!(html.contains("data-path=\"/api/v1/runs/run-000001/files/step-1.vtu\">"));
    let nonce = html
        .split("<script src=\"viewer.js\" nonce=\"")
        .nth(1)
        .expect("script");
    let nonce = &nonce[..nonce.find('"').unwrap()];
    assert!(
        html.contains(&format!("script-src 'nonce-{nonce}'")),
        "policy"
    );
}

/// A successful report retains its bytes beside the run, independent of the output path.
#[test]
fn a_report_retains_a_copy_beside_each_run() {
    let lab = Lab::new("report-artifact");
    let out = lab.report(&["--run", "run-000001"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let retained = lab.dir.join("state/runs/run-000001/report.html");
    let expected = fs::read(lab.out()).unwrap();
    assert_eq!(fs::read(&retained).unwrap(), expected);
    fs::remove_file(lab.out()).unwrap();
    assert_eq!(fs::read(retained).unwrap(), expected);
}
