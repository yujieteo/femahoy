//! Run the input of a case with `ccx` and read one number per reference from the text results.

use fh_solver_ccx::Ccx;
use fh_validation::{Case, Field, Op, Quantity, Solve};
use std::path::Path;
use std::time::Duration;

/// The time limit of one case.
const TIME_LIMIT: Duration = Duration::from_secs(300);

/// Runs `ccx` in a scratch folder on the input of a case.
pub struct Subprocess {
    pub solver: Ccx,
}

impl Subprocess {
    pub fn new() -> Self {
        let mut solver = Ccx::new();
        solver.timeout = Some(TIME_LIMIT);
        Subprocess { solver }
    }
}

impl Solve for Subprocess {
    fn measure(&self, case: &Case, input: &Path) -> Result<Vec<f64>, String> {
        let run =
            std::env::temp_dir().join(format!("fh-validate-{}-{}", std::process::id(), case.id));
        let _ = std::fs::remove_dir_all(&run);
        let result = self
            .solver
            .run(input, &run)
            .map_err(|e| format!("ccx: {e}"));
        let values = result.and_then(|()| {
            let file = run.join("ccx/job.dat");
            let dat =
                std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            case.references
                .iter()
                .map(|r| extract(&dat, &r.quantity))
                .collect()
        });
        let _ = std::fs::remove_dir_all(&run);
        values
    }
}

/// The kind of block of the `.dat` file, and the number of leading columns before the values.
fn block_of(field: Field) -> (&'static str, usize, usize) {
    match field {
        Field::U => ("displacements", 1, 3),
        Field::RF => ("forces", 1, 3),
        Field::S => ("stresses", 2, 6),
    }
}

/// The column of a component in the values of a row.
fn column_of(q: &Quantity) -> Result<usize, String> {
    let column = match (q.field, q.component) {
        (Field::U | Field::RF, c @ 1..=3) => c as usize - 1,
        (Field::S, 11) => 0,
        (Field::S, 22) => 1,
        (Field::S, 33) => 2,
        (Field::S, 12) => 3,
        (Field::S, 13) => 4,
        (Field::S, 23) => 5,
        (_, c) => return Err(format!("bad component {c}")),
    };
    Ok(column)
}

/// The rows of the last block of the field for the set, with the values of one component.
fn values_of(dat: &str, q: &Quantity) -> Result<Vec<f64>, String> {
    let (kind, lead, width) = block_of(q.field);
    let column = column_of(q)?;
    let mut block: Option<Vec<f64>> = None;
    let mut inside = false;
    for line in dat.lines() {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        let is_row = words.iter().all(|w| w.parse::<f64>().is_ok());
        if !is_row {
            // A header: `<kind> (...) for set NAME and time T`.
            let wanted = words.first().is_some_and(|w| *w == kind)
                && words
                    .iter()
                    .position(|w| *w == "set")
                    .and_then(|i| words.get(i + 1))
                    .is_some_and(|name| name.eq_ignore_ascii_case(&q.set));
            inside = wanted;
            if wanted {
                block = Some(Vec::new());
            }
            continue;
        }
        if !inside {
            continue;
        }
        if words.len() != lead + width {
            return Err(format!(
                "a row of {kind} for set `{}` has {} numbers, expected {}",
                q.set,
                words.len(),
                lead + width
            ));
        }
        let value: f64 = words[lead + column].parse().map_err(|_| "bad number")?;
        if let Some(rows) = block.as_mut() {
            rows.push(value);
        }
    }
    match block {
        None => Err(format!(
            "no {kind} for set `{}` in the .dat file. Add a print request for it to the input",
            q.set
        )),
        Some(rows) if rows.is_empty() => Err(format!("{kind} for set `{}` hold no rows", q.set)),
        Some(rows) => Ok(rows),
    }
}

/// Reduce the values of the set to the one number of the quantity. The `.dat` text holds the
/// values at the end of each increment that the print request asks for. The last block counts.
pub fn extract(dat: &str, q: &Quantity) -> Result<f64, String> {
    let picked = values_of(dat, q)?;
    // f64::max and f64::min drop a NaN, so the check comes before the reduction.
    if let Some(v) = picked.iter().find(|v| !v.is_finite()) {
        return Err(format!(
            "set `{}` has the value {v}, which is not finite",
            q.set
        ));
    }
    let value = match q.op {
        Op::Value if picked.len() == 1 => picked[0],
        Op::Value => {
            return Err(format!(
                "set `{}` holds {} rows, op value needs 1",
                q.set,
                picked.len()
            ))
        }
        Op::Mean => picked.iter().sum::<f64>() / picked.len() as f64,
        Op::Max => picked.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Op::Min => picked.iter().copied().fold(f64::INFINITY, f64::min),
        Op::Sum => picked.iter().sum(),
    };
    if !value.is_finite() {
        return Err(format!("the {:?} of set `{}` is not finite", q.op, q.set));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAT: &str = "
                        S T E P       1


                                INCREMENT     1


 forces (fx,fy,fz) for set SUPPORT and time  0.1000000E+01

         1 -1.000000E+01  0.000000E+00  0.000000E+00
         2 -3.000000E+01  0.000000E+00  0.000000E+00

 displacements (vx,vy,vz) for set LOAD and time  0.1000000E+01

         5  1.000000E+00  2.000000E+00  3.000000E+00
         6  3.000000E+00  4.000000E+00  5.000000E+00

 stresses (elem, integ.pnt.,sxx,syy,szz,sxy,sxz,syz) for set ALL and time  0.1000000E+01

         1   1  1.000000E+02  2.000000E+02  3.000000E+02  4.000000E+02  5.000000E+02  6.000000E+02
         1   2  3.000000E+02  2.000000E+02  3.000000E+02  4.000000E+02  5.000000E+02  6.000000E+02
";

    fn q(set: &str, field: Field, component: u32, op: Op) -> Quantity {
        Quantity {
            set: set.into(),
            field,
            component,
            op,
        }
    }

    #[test]
    fn each_field_and_op_reads_the_values_of_its_set() {
        let get = |set, field, c, op| extract(DAT, &q(set, field, c, op)).unwrap();
        assert_eq!(get("SUPPORT", Field::RF, 1, Op::Sum), -40.0);
        assert_eq!(get("support", Field::RF, 1, Op::Min), -30.0);
        assert_eq!(get("LOAD", Field::U, 2, Op::Mean), 3.0);
        assert_eq!(get("LOAD", Field::U, 3, Op::Max), 5.0);
        assert_eq!(get("ALL", Field::S, 11, Op::Mean), 200.0);
        assert_eq!(get("ALL", Field::S, 22, Op::Min), 200.0);
    }

    #[test]
    fn the_stress_components_follow_the_order_of_ccx() {
        let get = |c| extract(DAT, &q("ALL", Field::S, c, Op::Max)).unwrap();
        let all: Vec<f64> = [11, 22, 33, 12, 13, 23].into_iter().map(get).collect();
        assert_eq!(all, [300.0, 200.0, 300.0, 400.0, 500.0, 600.0]);
    }

    #[test]
    fn the_last_block_counts() {
        let two = format!("{DAT}\n{}", DAT.replace("-1.000000E+01", "-5.000000E+01"));
        let v = extract(&two, &q("SUPPORT", Field::RF, 1, Op::Sum)).unwrap();
        assert_eq!(v, -80.0);
    }

    #[test]
    fn a_missing_set_or_block_is_an_error_that_names_the_set() {
        let e = extract(DAT, &q("NOPE", Field::U, 1, Op::Mean)).unwrap_err();
        assert!(e.contains("NOPE"), "{e}");
        let e = extract(DAT, &q("SUPPORT", Field::U, 1, Op::Mean)).unwrap_err();
        assert!(e.contains("displacements"), "{e}");
        let e = extract(DAT, &q("LOAD", Field::U, 1, Op::Value)).unwrap_err();
        assert!(e.contains("needs 1"), "{e}");
    }

    #[test]
    fn a_value_that_is_not_finite_is_an_error_for_every_op() {
        let dat = DAT.replace("1.000000E+00  2.000000E+00", "NaN  2.000000E+00");
        for op in [Op::Mean, Op::Max, Op::Min, Op::Sum] {
            assert!(
                extract(&dat, &q("LOAD", Field::U, 1, op)).is_err(),
                "{op:?}"
            );
        }
        // Finite values with a sum that overflows.
        let dat = DAT.replace("3.000000E+00  4.000000E+00", "1e308  4.000000E+00");
        let dat = dat.replace("1.000000E+00  2.000000E+00", "1e308  2.000000E+00");
        assert!(extract(&dat, &q("LOAD", Field::U, 1, Op::Sum)).is_err());
    }

    #[test]
    fn a_row_with_the_wrong_number_of_columns_is_an_error_not_a_panic() {
        let dat = DAT.replace("5  1.000000E+00  2.000000E+00  3.000000E+00", "5  1.0  2.0");
        let r = std::panic::catch_unwind(|| extract(&dat, &q("LOAD", Field::U, 1, Op::Max)));
        assert!(matches!(r, Ok(Err(_))), "{r:?}");
    }
}
