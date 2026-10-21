//! Run the input of a case with `ccx`, or the OpenFOAM cases of a case with OpenFOAM, and read
//! one number per reference from the text results.

use std::path::Path;
use std::time::Duration;

use super::fixture::{Case, Field, Op, Quantity};
use super::run::Solve;
use crate::ccx::Ccx;
use crate::foam::{Foam, WORK};

/// The time limit of one case. The OpenFOAM cases of a case run at the same time, and each has
/// this limit.
const TIME_LIMIT: Duration = Duration::from_secs(300);

/// Runs `ccx` or OpenFOAM in a scratch folder on the input of a case.
pub struct Subprocess {
    solver: Ccx,
    foam: Foam,
}

impl Default for Subprocess {
    fn default() -> Self {
        let mut solver = Ccx::new();
        solver.timeout = Some(TIME_LIMIT);
        let mut foam = Foam::new();
        foam.timeout = Some(TIME_LIMIT);
        Subprocess { solver, foam }
    }
}

impl Solve for Subprocess {
    fn measure(&self, case: &Case, input: &Path) -> Result<Vec<f64>, String> {
        let run =
            std::env::temp_dir().join(format!("fh-validate-{}-{}", std::process::id(), case.id));
        let _ = std::fs::remove_dir_all(&run);
        let values = if case.openfoam() {
            self.coefficients(case, input, &run)
        } else {
            self.ccx(case, input, &run)
        };
        let _ = std::fs::remove_dir_all(&run);
        values
    }
}

impl Subprocess {
    fn ccx(&self, case: &Case, input: &Path, run: &Path) -> Result<Vec<f64>, String> {
        self.solver
            .run(input, run)
            .map_err(|e| format!("ccx: {e}"))?;
        let file = run.join("ccx/job.dat");
        let dat = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        case.references
            .iter()
            .map(|r| extract(&dat, &r.quantity))
            .collect()
    }

    /// Solve the OpenFOAM case of each check at the same time, each in its own folder in `run`,
    /// and read the force coefficients of the last iteration.
    fn coefficients(&self, case: &Case, input: &Path, run: &Path) -> Result<Vec<f64>, String> {
        let mut folders: Vec<&str> = case
            .references
            .iter()
            .map(|r| r.quantity.set.as_str())
            .collect();
        folders.sort_unstable();
        folders.dedup();
        let tables: Vec<Result<String, String>> = std::thread::scope(|scope| {
            let solves: Vec<_> = folders
                .iter()
                .map(|folder| {
                    scope.spawn(move || {
                        let dir = run.join(folder);
                        self.foam
                            .run(&input.join(folder), &dir)
                            .map_err(|e| format!("openfoam {folder}: {e}"))?;
                        let file = dir
                            .join(WORK)
                            .join("postProcessing/forceCoeffs/0/coefficient.dat");
                        std::fs::read_to_string(&file)
                            .map_err(|e| format!("{}: {e}", file.display()))
                    })
                })
                .collect();
            solves
                .into_iter()
                .map(|s| {
                    s.join()
                        .unwrap_or_else(|_| Err("the solve panicked".into()))
                })
                .collect()
        });
        case.references
            .iter()
            .map(|r| {
                let folder = r.quantity.set.as_str();
                let at = folders
                    .binary_search(&folder)
                    .expect("each folder is in the list");
                let table = tables[at].as_ref().map_err(Clone::clone)?;
                let name = r.quantity.field.coefficient().expect("a force coefficient");
                coefficient(table, name).map_err(|e| format!("{folder}: coefficient.dat: {e}"))
            })
            .collect()
    }
}

/// The column `name` of the last row of the `coefficient.dat` text of `forceCoeffs`. The last
/// comment line names the columns: `# Time Cd Cd(f) Cd(r) Cl ...`.
fn coefficient(text: &str, name: &str) -> Result<f64, String> {
    let header = text
        .lines()
        .rfind(|l| l.starts_with('#'))
        .ok_or("no header line")?;
    let column = header[1..]
        .split_whitespace()
        .position(|w| w == name)
        .ok_or_else(|| format!("no column {name}"))?;
    let row = text
        .lines()
        .rfind(|l| !l.starts_with('#') && !l.trim().is_empty())
        .ok_or("no row")?;
    let value: f64 = row
        .split_whitespace()
        .nth(column)
        .and_then(|w| w.parse().ok())
        .ok_or_else(|| format!("the last row has no number in column {name}"))?;
    if !value.is_finite() {
        return Err(format!("{name} is {value}, which is not finite"));
    }
    Ok(value)
}

/// The kind of block of the `.dat` file, and the number of leading columns before the values.
fn block_of(field: Field) -> (&'static str, usize, usize) {
    match field {
        Field::U => ("displacements", 1, 3),
        Field::RF => ("forces", 1, 3),
        Field::S => ("stresses", 2, 6),
        Field::NT => ("temperatures", 1, 1),
        Field::Cstr => ("contact stress", 2, 3),
        // `mode_value` and `coefficient` read these. They have no block of a set.
        Field::Freq | Field::Buckle | Field::Cl | Field::Cd | Field::Cm => ("", 0, 0),
    }
}

/// The column of a component in the values of a row.
fn column_of(q: &Quantity) -> Result<usize, String> {
    let column = match (q.field, q.component) {
        (Field::U | Field::RF | Field::Cstr, c @ 1..=3) => c as usize - 1,
        (Field::NT, 1) => 0,
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
/// A block with no set (`for all contact elements`) is the block of the empty set name.
fn values_of(dat: &str, q: &Quantity) -> Result<Vec<f64>, String> {
    let (kind, lead, width) = block_of(q.field);
    let column = column_of(q)?;
    let of = match q.set.as_str() {
        "" => String::new(),
        set => format!(" for set `{set}`"),
    };
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
            let set = words
                .iter()
                .position(|w| *w == "set")
                .and_then(|i| words.get(i + 1))
                .map_or("", |name| name);
            let wanted = text.starts_with(&format!("{kind} ")) && set.eq_ignore_ascii_case(&q.set);
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
                "a row of {kind}{of} has {} numbers, expected {}",
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
            "no {kind}{of} in the .dat file. Add a print request for it to the input"
        )),
        Some(rows) if rows.is_empty() => Err(format!("{kind}{of} hold no rows")),
        Some(rows) => Ok(rows),
    }
}

/// The value in column `column` of the row of `mode` in the last table `title` of the `.dat`
/// text. A row has `width` numbers, and the first is the mode. The table ends at the first line
/// after its rows that is not a row.
fn mode_value(
    dat: &str,
    title: &str,
    width: usize,
    column: usize,
    mode: u32,
) -> Result<f64, String> {
    let mut table: Option<Vec<Vec<f64>>> = None;
    let mut inside = false;
    for line in dat.lines().filter(|l| !l.trim().is_empty()) {
        let row: Option<Vec<f64>> = line.split_whitespace().map(|w| w.parse().ok()).collect();
        match (row, table.as_mut()) {
            _ if line.contains(title) => {
                table = Some(Vec::new());
                inside = true;
            }
            (Some(row), Some(rows)) if inside && row.len() == width => rows.push(row),
            (_, Some(rows)) if !rows.is_empty() => inside = false,
            _ => {}
        }
    }
    let rows = table.ok_or_else(|| format!("no table `{title}` in the .dat file"))?;
    rows.iter()
        .find(|r| r[0] == f64::from(mode))
        .map(|r| r[column])
        .ok_or_else(|| format!("the table `{title}` has no mode {mode}"))
}

/// Reduce the values of the set to the one number of the quantity. The `.dat` text holds the
/// values at the end of each increment that the print request asks for. The last block counts.
/// A frequency or a buckling factor comes from the last table of its step.
fn extract(dat: &str, q: &Quantity) -> Result<f64, String> {
    match q.field {
        Field::Freq => {
            let title = "E I G E N V A L U E   O U T P U T";
            return mode_value(dat, title, 5, 3, q.component);
        }
        Field::Buckle => {
            let title = "B U C K L I N G   F A C T O R   O U T P U T";
            return mode_value(dat, title, 2, 1, q.component);
        }
        _ => {}
    }
    let picked = values_of(dat, q)?;
    // f64::max drops a NaN, so the check comes before the reduction.
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
        Op::Sum => picked.iter().sum(),
    };
    if !value.is_finite() {
        return Err(format!("the {:?} of set `{}` is not finite", q.op, q.set));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::coefficient;

    const TABLE: &str = "# Force and moment coefficients\n# magUInf : 1\n#\n\
        # Time    \tCd    \tCd(f)  \tCd(r) \tCl    \tCmPitch\n\
        1         \t0.75  \t-0.35  \t1.1   \t1.47  \t0.2    \n\
        1500      \t0.0094\t0.004  \t0.005 \t0.4448\t-0.1151\n";

    #[test]
    fn a_coefficient_is_the_column_of_its_name_in_the_last_row() {
        assert_eq!(coefficient(TABLE, "Cl"), Ok(0.4448));
        assert_eq!(coefficient(TABLE, "Cd"), Ok(0.0094));
        assert_eq!(coefficient(TABLE, "CmPitch"), Ok(-0.1151));
        assert!(coefficient(TABLE, "Cs")
            .unwrap_err()
            .contains("no column Cs"));
        let cut = TABLE.replace("\t-0.1151", "");
        assert!(coefficient(&cut, "CmPitch")
            .unwrap_err()
            .contains("no number"));
        assert!(coefficient("# Time Cd\n", "Cd")
            .unwrap_err()
            .contains("no row"));
        assert!(coefficient("# Time Cd\n1 nan\n", "Cd")
            .unwrap_err()
            .contains("not finite"));
    }
}
