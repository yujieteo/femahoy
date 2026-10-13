//! STEP unit scan. It reads the length-unit record of a STEP file as text.
//! Decisions D-12 and D-13, see DESIGN.md: gmsh does the geometry, `fh` checks the unit.

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

impl StepUnit {
    /// The value of the gmsh option `Geometry.OCCTargetUnit` that equals this unit.
    pub fn gmsh_name(&self) -> Option<&'static str> {
        match self {
            StepUnit::Mm => Some("MM"),
            StepUnit::M => Some("M"),
            StepUnit::Inch => Some("INCH"),
            StepUnit::Other(_) => None,
        }
    }
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

/// Why the scan gives no single length unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepUnitError {
    /// The file has no length-unit record.
    Missing,
    /// The file has length-unit records that differ.
    Conflict(Vec<StepUnit>),
}

impl StepUnitError {
    /// The stable refusal code.
    pub fn code(&self) -> &'static str {
        match self {
            StepUnitError::Missing => "unit.step.missing",
            StepUnitError::Conflict(_) => "unit.step.conflict",
        }
    }
}

impl fmt::Display for StepUnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepUnitError::Missing => f.write_str("the STEP file has no length-unit record"),
            StepUnitError::Conflict(units) => {
                let names: Vec<String> = units.iter().map(StepUnit::to_string).collect();
                write!(
                    f,
                    "the STEP file has length units that differ: {}",
                    names.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for StepUnitError {}

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
pub fn step_length_unit(text: &str) -> Result<StepUnit, StepUnitError> {
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
        0 => Err(StepUnitError::Missing),
        1 => Ok(units.remove(0)),
        _ => Err(StepUnitError::Conflict(units)),
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
    fn reads_a_record_that_spans_lines() {
        let text = step("#9=(\n  LENGTH_UNIT()\n  NAMED_UNIT(*)\n  SI_UNIT(.MILLI.,.METRE.)\n);");
        assert_eq!(step_length_unit(&text), Ok(StepUnit::Mm));
    }

    #[test]
    fn ignores_angle_units_and_comments() {
        let text = step(
            "/* LENGTH_UNIT() SI_UNIT($,.METRE.) */\n#8=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n#9=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));",
        );
        assert_eq!(step_length_unit(&text), Ok(StepUnit::Mm));
    }

    #[test]
    fn other_units_keep_their_name() {
        let cm = step("#9=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.));");
        assert_eq!(
            step_length_unit(&cm),
            Ok(StepUnit::Other("centimetre".to_owned()))
        );
        assert_eq!(StepUnit::Other("x".into()).gmsh_name(), None);
    }

    #[test]
    fn no_record_is_an_error() {
        let text = step("#9=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));");
        assert_eq!(step_length_unit(&text), Err(StepUnitError::Missing));
        assert_eq!(StepUnitError::Missing.code(), "unit.step.missing");
    }

    #[test]
    fn records_that_differ_are_an_error() {
        let text = step(
            "#8=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#9=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));",
        );
        let err = step_length_unit(&text).unwrap_err();
        assert_eq!(err.code(), "unit.step.conflict");
    }
}
