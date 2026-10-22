//! `fh fit`: the fit of a model to a curve file, as the agent runs it.

use std::fs;
use std::process::{Command, Output};

mod common;
use common::{p, scratch};

fn fh(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .arg("fit")
        .args(args)
        .output()
        .expect("run fh")
}

/// The nominal stress of a Mooney-Rivlin solid with C10 = 0.2 and C01 = 0.05 MPa.
const TEST: &str = "# y: P [MPa]\n# points test: a test\nstretch [1],test\n1,0\n1.5,0.4925926\n\
                    2,0.7875\n3,1.2518519\n";

/// `fh fit` prints the constants, the goodness of fit, the range and the CalculiX lines, and
/// `-o` writes the curve with the fit for `fh report`.
#[test]
fn fit_prints_the_constants_and_writes_the_curve() {
    let dir = scratch("fit");
    let (curve, out) = (dir.join("test.csv"), dir.join("fit.csv"));
    fs::write(&curve, TEST).unwrap();
    let run = fh(&["mooney-rivlin", p(&curve), "-o", p(&out)]);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    for text in [
        "C10 = 2.0000",
        "C01 = 4.99999",
        "R^2 = ",
        "largest residual = ",
        "valid for x from 1 to 3 [1]",
        "*HYPERELASTIC, MOONEY-RIVLIN\n2.0000",
    ] {
        assert!(stdout.contains(text), "{text} in\n{stdout}");
    }
    let written = fs::read_to_string(&out).unwrap();
    assert!(
        written.contains("stretch [1],test,mooney-rivlin fit\n"),
        "{written}"
    );
    assert!(
        written.contains("# note: mooney-rivlin fit of test: C10 = "),
        "{written}"
    );
}

#[test]
fn a_bad_model_or_series_is_an_error() {
    let dir = scratch("fit-bad");
    let curve = dir.join("test.csv");
    fs::write(&curve, TEST).unwrap();
    let run = fh(&["ogden9", p(&curve)]);
    assert_eq!(run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run.stderr).contains("unknown model `ogden9`"));
    let run = fh(&["poly1", p(&curve), "--series", "other"]);
    assert_eq!(run.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("no series `other`. The series are: test")
    );
}

/// `-o` writes no curve that `fh report` cannot read: a fit at or below 0 on a log y axis.
#[test]
fn a_fit_at_or_below_0_on_a_log_y_axis_is_an_error() {
    let dir = scratch("fit-log-y");
    let (curve, out) = (dir.join("test.csv"), dir.join("fit.csv"));
    fs::write(&curve, "# y: a [1]\n# log: y\nx [1],test\n1,10\n2,1\n3,0.5\n").unwrap();
    let run = fh(&["poly1", p(&curve), "-o", p(&out)]);
    assert_eq!(run.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("the log axis y needs values above 0"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(!out.exists());
}
