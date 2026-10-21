//! STEP unit scan. It reads the length-unit record of a STEP file as text.
//! gmsh does the geometry, and `fh` checks the unit.

use std::fmt;

/// The length unit that a STEP file declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepUnit {
    Mm,
    M,
    Inch,
    /// Any other unit, for example `CENTI` or `FOOT`. `fh` refuses it.
    Other(String),
}

impl fmt::Display for StepUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepUnit::Mm => f.write_str("mm"),
            StepUnit::M => f.write_str("m"),
            StepUnit::Inch => f.write_str("inch"),
            StepUnit::Other(name) => f.write_str(name),
        }
    }
}

/// Remove `/* ... */` comments and all white space outside quotes.
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..].split_once("*/").map_or("", |(_, r)| r);
    }
    out.push_str(rest);
    let mut clean = String::with_capacity(out.len());
    let mut quoted = false;
    for c in out.chars() {
        if c == '\'' {
            quoted = !quoted;
        }
        if quoted || !c.is_whitespace() {
            clean.push(c);
        }
    }
    clean
}

/// The unit of one entity text with `LENGTH_UNIT()` in it.
fn entity_unit(entity: &str) -> Option<StepUnit> {
    let upper = entity.to_ascii_uppercase();
    if !upper.contains("LENGTH_UNIT()") {
        return None;
    }
    if let Some(at) = upper.find("CONVERSION_BASED_UNIT('") {
        let name = upper[at + 23..].split('\'').next().unwrap_or("");
        return Some(match name {
            "INCH" | "INCHES" => StepUnit::Inch,
            other => StepUnit::Other(other.to_owned()),
        });
    }
    let args = upper.split_once("SI_UNIT(")?.1.split_once(')')?.0;
    let (prefix, unit) = args.split_once(',')?;
    if unit != ".METRE." {
        return None;
    }
    Some(match prefix {
        "$" => StepUnit::M,
        ".MILLI." => StepUnit::Mm,
        other => StepUnit::Other(format!("{}metre", other.trim_matches('.').to_lowercase())),
    })
}

/// Read the length unit of a STEP file. The file must hold one length unit, or the same unit
/// in each record. A missing record is an error, because gmsh does not convert in that case.
pub fn step_length_unit(text: &str) -> Result<StepUnit, String> {
    let data = normalise(text);
    let mut units: Vec<StepUnit> = Vec::new();
    for entity in data.split(';') {
        if let Some(unit) = entity_unit(entity) {
            if !units.contains(&unit) {
                units.push(unit);
            }
        }
    }
    match units.len() {
        0 => Err("the STEP file has no length-unit record".to_owned()),
        1 => Ok(units.remove(0)),
        _ => {
            let names: Vec<String> = units.iter().map(StepUnit::to_string).collect();
            Err(format!(
                "the STEP file has length units that differ: {}",
                names.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(unit_entity: &str) -> String {
        format!("ISO-10303-21;\nDATA;\n#1=PRODUCT('a','a','',(#2));\n{unit_entity}\nENDSEC;\nEND-ISO-10303-21;\n")
    }

    #[test]
    fn reads_milli_metre_metre_and_inch() {
        let mm = step("#9=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );");
        let m = step("#9=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));");
        let inch = step("#9=(CONVERSION_BASED_UNIT('INCH',#8)LENGTH_UNIT()NAMED_UNIT(#7));");
        assert_eq!(step_length_unit(&mm), Ok(StepUnit::Mm));
        assert_eq!(step_length_unit(&m), Ok(StepUnit::M));
        assert_eq!(step_length_unit(&inch), Ok(StepUnit::Inch));
    }

    #[test]
    fn ignores_angle_units_and_comments() {
        let text = step(
            "/* LENGTH_UNIT() SI_UNIT($,.METRE.) */\n#8=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n#9=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));",
        );
        assert_eq!(step_length_unit(&text), Ok(StepUnit::Mm));
    }

    #[test]
    fn records_that_differ_are_an_error() {
        let text = step(
            "#8=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#9=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));",
        );
        let err = step_length_unit(&text).unwrap_err();
        assert!(err.contains("length units that differ"), "{err}");
    }
}
