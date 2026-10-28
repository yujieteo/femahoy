//! Tests for the pass rule, the check file and the shipped cases.

use fh::validation::{check, Case, Field};
use std::path::PathBuf;

fn write_case(dir: &std::path::Path, body: &str) -> Result<Case, String> {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("check.toml"), body).unwrap();
    std::fs::write(dir.join("job.inp"), "*NODE\n1, 0, 0, 0\n").unwrap();
    Case::load(dir)
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fh-validation-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A check file: `extra` top-level keys after the tolerances, then the check `q`.
fn body(extra: &str) -> String {
    format!(
        "rel_tol = 1\nabs_tol = 0\n{extra}\n[q]\nvalue = 1.0\nset = \"s\"\nfield = \"U\"\ncomponent = 1\nop = \"mean\"\n"
    )
}

#[test]
fn the_pass_rule_uses_relative_plus_absolute_tolerance() {
    assert!(check(1.0000005, 1.0, 1e-6, 0.0).is_ok());
    assert!(check(1.00001, 1.0, 1e-6, 0.0).is_err());
    assert!(check(1e-12, 0.0, 0.0, 1e-9).is_ok());
}

#[test]
fn the_pass_rule_rejects_infinite_tolerance() {
    assert!(check(999.0, 1.0, f64::INFINITY, 0.0).is_err());
}

#[test]
fn the_pass_rule_rejects_infinite_result() {
    assert!(check(f64::INFINITY, 1.0, f64::INFINITY, 0.0).is_err());
}

#[test]
fn the_pass_rule_rejects_invalid_numbers_and_negative_tolerances() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..4 {
            let mut args = [1.0, 1.0, 0.0, 0.0];
            args[index] = bad;
            assert!(check(args[0], args[1], args[2], args[3]).is_err());
        }
    }
    assert!(check(1.0, 1.0, -1.0, 1.0).is_err());
    assert!(check(1.0, 1.0, 1.0, -1.0).is_err());
    assert!(check(1.0, 1.0, 0.0, 0.0).is_ok());
}

#[test]
fn the_pass_rule_rejects_non_finite_arithmetic() {
    assert!(check(1.0, f64::MAX, 2.0, 0.0).is_err());
    assert!(check(f64::MAX, -f64::MAX, 1.0, f64::MAX).is_err());
}

#[test]
fn a_number_that_is_not_finite_is_refused() {
    let dir = tmp("invalid-numbers");
    let mut index = 0;
    for (key, line) in [("rel_tol", 1), ("abs_tol", 2), ("value", 5)] {
        for bad in ["inf", "-inf", "nan"] {
            let text: Vec<String> = body("")
                .lines()
                .map(|l| {
                    if l.starts_with(&format!("{key} =")) {
                        format!("{key} = {bad}")
                    } else {
                        l.to_owned()
                    }
                })
                .collect();
            index += 1;
            let error = write_case(&dir.join(format!("bad{index}")), &text.join("\n")).unwrap_err();
            assert!(error.contains(key), "{key}: {error}");
            assert!(error.contains(&format!("check.toml:{line}: ")), "{error}");
        }
    }
}

/// A check with `case` reads a column of a table in the subfolder of an OpenFOAM, GetDP or LAMMPS
/// case. The case has no `job.inp`.
#[test]
fn a_table_check_names_an_openfoam_getdp_or_lammps_case() {
    let dir = tmp("openfoam");
    std::fs::create_dir_all(dir.join("a4/system")).unwrap();
    std::fs::write(
        dir.join("a4/system/controlDict"),
        "application simpleFoam;\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("em")).unwrap();
    std::fs::write(dir.join("em/case.pro"), "").unwrap();
    std::fs::create_dir_all(dir.join("sph")).unwrap();
    std::fs::write(dir.join("sph/case.lmp"), "").unwrap();
    let check = |name: &str, case: &str, file: &str| {
        format!(
            "[{name}]\nvalue = 0.4\ncase = \"{case}\"\nfile = \"{file}\"\ncolumn = \"{name}\"\n"
        )
    };
    let coefficients = "postProcessing/forceCoeffs/0/coefficient.dat";
    let load = |body: &str| {
        std::fs::write(
            dir.join("check.toml"),
            format!("rel_tol = 0.1\nabs_tol = 0\n{body}"),
        )
        .unwrap();
        Case::load(&dir)
    };
    let case = load(
        &(check("Cl", "a4", coefficients)
            + &check("Cd", "em", "b.txt")
            + &check("Z", "sph", "front.txt")),
    )
    .unwrap();
    assert!(case.tables());
    assert_eq!(case.references[2].quantity.set, "sph");
    assert_eq!(case.input, dir);
    assert_eq!(case.references[1].quantity.set, "em");
    assert_eq!(
        case.references[1].quantity.field,
        Field::Table {
            file: "b.txt".into(),
            column: "Cd".into(),
            row: None,
        }
    );

    let missing = load(&check("Cl", "a10", coefficients)).unwrap_err();
    assert!(
        missing.contains("a10/system/controlDict, ")
            && missing.contains("a10/openems.py, ")
            && missing.contains("a10/precice-config.xml or ")
            && missing.contains("a10/case.lmp not found"),
        "{missing}"
    );
    let outside = load(&check("Cl", "../a4", coefficients)).unwrap_err();
    assert!(
        outside.contains("check.toml:5: `case` must be the name of a subfolder"),
        "{outside}"
    );
    for file in ["../a10/p.dat", "/etc/p.dat", "postProcessing//p.dat", ""] {
        let error = load(&check("Cl", "a4", file)).unwrap_err();
        assert!(
            error.contains("check.toml:6: `file` must"),
            "{file}: {error}"
        );
    }
    let mixed = check("Cl", "a4", coefficients)
        + "[u]\nvalue = 1\nset = \"s\"\nfield = \"U\"\ncomponent = 1\nop = \"mean\"\n";
    let mixed = load(&mixed).unwrap_err();
    assert!(mixed.contains("table checks (with `case`) only"), "{mixed}");
    let extra = load(&(check("Cd", "a4", coefficients) + "field = \"CD\"\n")).unwrap_err();
    assert!(extra.contains("unknown key `field` in `Cd`"), "{extra}");
    let no_column = load("[Cd]\nvalue = 0.4\ncase = \"a4\"\nfile = \"p.dat\"\n").unwrap_err();
    assert!(no_column.contains("missing key `column`"), "{no_column}");
    let _ = std::fs::remove_dir_all(&dir);
}
