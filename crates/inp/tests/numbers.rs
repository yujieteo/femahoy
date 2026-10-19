//! Every number field rejects NaN, infinity and numbers that overflow to infinity. The Rust
//! number parser accepts all of them, and a NaN or infinite support value or load changes the
//! model in the solver without an error.

use fh_inp::parse;

const MESH: &str =
    "*NODE\n1,0,0,0\n2,1,0,0\n3,0,1,0\n4,0,0,1\n5,.5,0,0\n6,.5,.5,0\n7,0,.5,0\n8,0,0,.5\n\
9,.5,0,.5\n10,0,.5,.5\n*ELEMENT, TYPE=C3D10, ELSET=E\n1,1,2,3,4,5,6,7,8,9,10\n";
const MATERIAL: &str =
    "*MATERIAL, NAME=M\n*ELASTIC\n1000,0.3\n*SOLID SECTION, ELSET=E, MATERIAL=M\n";

const BAD: [&str; 8] = [
    "NaN",
    "nan",
    "inf",
    "-inf",
    "infinity",
    "+Infinity",
    "1e999",
    "-1e999",
];

/// The keyword and the model text. `@` marks the number field under test.
fn cases() -> Vec<(&'static str, String)> {
    let step = |data: &str| format!("{MESH}{MATERIAL}*STEP\n*STATIC\n{data}*END STEP\n");
    vec![
        ("NODE", format!("{MESH}*NODE\n11,0,@,0\n")),
        (
            "ELASTIC",
            format!("{MESH}*MATERIAL, NAME=M\n*ELASTIC\n@,0.3\n"),
        ),
        (
            "ELASTIC",
            format!("{MESH}*MATERIAL, NAME=M\n*ELASTIC\n1000,@\n"),
        ),
        ("BOUNDARY", format!("{MESH}{MATERIAL}*BOUNDARY\n1,1,1,@\n")),
        ("BOUNDARY", step("*BOUNDARY\n1,1,1,@\n")),
        ("CLOAD", step("*CLOAD\n7,1,@\n")),
        ("DLOAD", step("*DLOAD\nE,P1,@\n")),
    ]
}

#[test]
fn the_cases_parse_with_a_finite_number() {
    for (_, text) in cases() {
        parse(&text.replace('@', "0.25")).unwrap_or_else(|e| panic!("{e}\n{text}"));
    }
}

#[test]
fn nan_infinity_and_overflow_are_errors_with_keyword_line_and_token() {
    for (keyword, text) in cases() {
        let line = text.lines().position(|l| l.contains('@')).unwrap() + 1;
        for bad in BAD {
            let t = text.replace('@', bad);
            let e = match parse(&t) {
                Ok(_) => panic!("*{keyword} line {line}: '{bad}' was accepted"),
                Err(e) => e.to_string(),
            };
            assert!(e.starts_with(&format!("line {line}, *{keyword}: ")), "{e}");
            assert!(e.contains(&format!("'{bad}'")), "{e}");
            assert!(e.contains("finite number"), "{e}");
        }
    }
}
