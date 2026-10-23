//! `fh report`: one HTML file with the text of the agent, the wasm viewer and the results of the
//! runs that the agent names.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

mod common;
use common::{p, root, scratch};

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

    /// Give `run-000001` the results and the input of the cantilever with 2 steps of
    /// `fixtures/results`, and write `view` to `view.toml`.
    fn cantilever(&self, view: &str) -> PathBuf {
        let fixture = root().join("fixtures/results/cantilever-2step");
        let run = self.dir.join("state/runs/run-000001");
        for file in ["results.pvd", "step-1.vtu", "step-2.vtu"] {
            fs::copy(fixture.join(file), run.join("out").join(file)).unwrap();
        }
        fs::copy(
            fixture.join("cantilever-2step.inp"),
            run.join("in/input.inp"),
        )
        .unwrap();
        let path = self.dir.join("view.toml");
        fs::write(&path, view).unwrap();
        path
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

/// A report with no run and no curve is the text only, with no viewer.
#[test]
fn a_report_of_the_text_only_needs_no_run_and_no_curve() {
    let lab = Lab::new("report-text-only");
    fs::remove_dir_all(lab.dir.join("viewer")).unwrap();
    let out = lab.report(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert!(html.contains("<h1>A320 barrel</h1>"), "text");
    assert!(!html.contains("data-path="), "no files");
}

/// With `--one-step` and no view, the report holds only the last step of the run, and its
/// `results.pvd` names only that step.
#[test]
fn the_one_step_option_keeps_only_the_last_step() {
    let lab = Lab::new("report-one-step");
    let out = lab.dir.join("state/runs/run-000001/out");
    fs::write(
        out.join("results.pvd"),
        "<VTKFile type=\"Collection\"><Collection units=\"SI-mm\">\
         <DataSet timestep=\"1\" part=\"0\" file=\"step-1.vtu\"/>\
         <DataSet timestep=\"2\" part=\"0\" file=\"step-2.vtu\"/></Collection></VTKFile>",
    )
    .unwrap();
    fs::write(out.join("step-2.vtu"), b"last vtu").unwrap();
    let out = lab.report(&["--run", "run-000001", "--one-step"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    let base = "/api/v1/runs/run-000001/files";
    assert_eq!(file(&html, &format!("{base}/step-1.vtu")), None);
    assert_eq!(
        file(&html, &format!("{base}/step-2.vtu")).as_deref(),
        Some(&b"last vtu"[..])
    );
    let pvd = String::from_utf8(file(&html, &format!("{base}/results.pvd")).unwrap()).unwrap();
    assert_eq!(
        pvd,
        "<VTKFile type=\"Collection\"><Collection units=\"SI-mm\">\
         <DataSet timestep=\"2\" part=\"0\" file=\"step-2.vtu\"/></Collection></VTKFile>"
    );
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

/// The view of a run is in the report under `/views/RUN`, as the text that the viewer reads.
#[test]
fn the_report_holds_the_view_of_a_run() {
    let lab = Lab::new("report-view");
    let view = lab.cantilever(
        "[run-000001]\nfields = [\"U3\", \"MISES\"]\ncontrols = [\"deformation\", \"loads\"]\n\
         step = 1\nscale = 1\n",
    );
    let out = lab.report(&["--run", "run-000001", "--view", p(&view)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    assert_eq!(
        String::from_utf8(file(&html, "/views/run-000001").unwrap()).unwrap(),
        "field=U3\nfield=MISES\ncontrols=deformation,loads\nstep=1\nscale=1\n"
    );
    // With --one-step, the report keeps the step of the view, and the view names no step.
    let out = lab.report(&["--run", "run-000001", "--view", p(&view), "--one-step"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    let base = "/api/v1/runs/run-000001/files";
    assert!(
        file(&html, &format!("{base}/step-1.vtu")).is_some(),
        "step 1"
    );
    assert_eq!(file(&html, &format!("{base}/step-2.vtu")), None, "step 2");
    let text = String::from_utf8(file(&html, "/views/run-000001").unwrap()).unwrap();
    assert_eq!(
        text,
        "field=U3\nfield=MISES\ncontrols=deformation,loads\nscale=1\n"
    );
}

/// With `--one-step`, a view with steps in its controls gives a report. The view has no steps
/// in its controls, and the text tells why.
#[test]
fn the_one_step_option_removes_steps_from_the_controls() {
    let lab = Lab::new("report-one-step-controls");
    let view = lab.cantilever("[run-000001]\ncontrols = [\"steps\", \"deformation\"]\n");
    let out = lab.report(&["--run", "run-000001", "--view", p(&view), "--one-step"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let html = fs::read_to_string(lab.out()).unwrap();
    let text = String::from_utf8(file(&html, "/views/run-000001").unwrap()).unwrap();
    assert_eq!(text, "controls=deformation\n");
    assert!(
        html.contains("Only 1 step of run-000001 is in this report"),
        "note"
    );
}

/// A view that does not fit its run gives no report, and the message tells why.
#[test]
fn a_view_that_does_not_fit_the_run_gives_no_report() {
    let lab = Lab::new("report-bad-view");
    for (view, want) in [
        (
            "fields = [\"Ez\"]",
            "[run-000001]: field \"Ez\": the run has no such field",
        ),
        ("step = 3", "[run-000001]: step 3: the run has 2 steps"),
        ("controls = [\"flow\"]", "no control \"flow\""),
        ("colour = \"red\"", "no key `colour`"),
    ] {
        let path = lab.cantilever(&format!("[run-000001]\n{view}\n"));
        let args = ["--run", "run-000001", "--view", p(&path)];
        let _ = fs::remove_file(lab.out());
        let out = lab.report(&args);
        assert_eq!(out.status.code(), Some(1), "{view}: {}", stderr(&out));
        assert!(stderr(&out).contains(want), "{view}: {}", stderr(&out));
        assert!(!lab.out().exists(), "{view}");
    }
    let path = lab.cantilever("[run-000002]\nstep = 1\n");
    let out = lab.report(&["--run", "run-000001", "--view", p(&path)]);
    assert!(
        stderr(&out).contains("[run-000002]: no --run names this run"),
        "{}",
        stderr(&out)
    );
}
