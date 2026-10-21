//! Run the input of a case with `ccx`, or the OpenFOAM, GetDP, preCICE, Chrono and topology cases
//! of a case with their programs and the topology loop, and read one number per reference from the
//! text results.

use std::path::Path;

use super::fixture::{Case, Field, Op, Quantity};
use super::run::Solve;
use crate::ccx::Ccx;
use crate::chrono::{self, Chrono};
use crate::foam::{self, Foam};
use crate::getdp::{self, GetDp};
use crate::precice::{self, Precice};
use crate::topology;

/// Runs `ccx`, OpenFOAM, GetDP, preCICE, Chrono or the topology loop in a scratch folder on the
/// input of a case.
#[derive(Default)]
pub struct Subprocess {
    solver: Ccx,
    foam: Foam,
    getdp: GetDp,
    chrono: Chrono,
    precice: Precice,
}

impl Solve for Subprocess {
    fn measure(&self, case: &Case, input: &Path) -> Result<Vec<f64>, String> {
        let run =
            std::env::temp_dir().join(format!("fh-validate-{}-{}", std::process::id(), case.id));
        let _ = std::fs::remove_dir_all(&run);
        let values = if case.tables() {
            self.tables(case, input, &run)
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

    /// Solve the OpenFOAM, GetDP, preCICE, Chrono or topology case of each check at the same time,
    /// each in its own folder in `run`, and read the value of each check in the last row of its
    /// table.
    fn tables(&self, case: &Case, input: &Path, run: &Path) -> Result<Vec<f64>, String> {
        let mut folders: Vec<&str> = case
            .references
            .iter()
            .map(|r| r.quantity.set.as_str())
            .collect();
        folders.sort_unstable();
        folders.dedup();
        // Each solve gives the folder of the solver files in its run folder.
        let solves: Vec<Result<&str, String>> = std::thread::scope(|scope| {
            let solves: Vec<_> = folders
                .iter()
                .map(|folder| {
                    scope.spawn(move || {
                        let (dir, out) = (input.join(folder), run.join(folder));
                        if getdp::is_case(&dir) {
                            let solved = self.getdp.run(&dir, &out);
                            solved
                                .map(|()| getdp::WORK)
                                .map_err(|e| format!("getdp {folder}: {e}"))
                        } else if topology::is_case(&dir) {
                            let cancel = crate::ccx::CancelToken::new();
                            let solved =
                                topology::run_with(&self.solver, &dir, &out, &cancel, &mut |_| {});
                            solved
                                .map(|()| topology::WORK)
                                .map_err(|e| format!("topology {folder}: {e}"))
                        } else if chrono::is_case(&dir) {
                            let solved = self.chrono.run(&dir, &out);
                            solved
                                .map(|()| chrono::WORK)
                                .map_err(|e| format!("chrono {folder}: {e}"))
                        } else if precice::is_case(&dir) {
                            let solved = self.precice.run(&dir, &out);
                            solved
                                .map(|()| precice::WORK)
                                .map_err(|e| format!("precice {folder}: {e}"))
                        } else {
                            let solved = self.foam.run(&dir, &out);
                            solved
                                .map(|()| foam::WORK)
                                .map_err(|e| format!("openfoam {folder}: {e}"))
                        }
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
                let work = solves[at].clone()?;
                let Field::Table { file, column } = &r.quantity.field else {
                    unreachable!("a case with tables has table checks only")
                };
                let path = run.join(folder).join(work).join(file);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                last_row(&text, column).map_err(|e| format!("{folder}: {file}: {e}"))
            })
            .collect()
    }
}

/// The value in column `name` of the last row of the table of a function object, for example
/// `coefficient.dat` of `forceCoeffs`. The last comment line names the columns:
/// `# Time Cd Cd(f) Cd(r) Cl ...`. A table with no comment line, for example a watch point of
/// preCICE, names the columns in its first line. Each column holds one number in each row.
fn last_row(text: &str, name: &str) -> Result<f64, String> {
    let header = text
        .lines()
        .rfind(|l| l.starts_with('#'))
        .or_else(|| text.lines().find(|l| !l.trim().is_empty()))
        .ok_or("no header line")?;
    let names: Vec<&str> = header.trim_start_matches('#').split_whitespace().collect();
    let column = names
        .iter()
        .position(|w| *w == name)
        .ok_or_else(|| format!("no column {name}"))?;
    let row: Vec<&str> = text
        .lines()
        .rfind(|l| !l.starts_with('#') && !l.trim().is_empty() && !std::ptr::eq(*l, header))
        .ok_or("no row")?
        .split_whitespace()
        .collect();
    // A vector, for example `(1 0 0)`, is 3 words in 1 column.
    if row.len() != names.len() {
        return Err(format!(
            "the last row has {} words for {} columns. Write each vector field as its components \
             with the function object `components`",
            row.len(),
            names.len()
        ));
    }
    let value: f64 = row[column]
        .parse()
        .map_err(|_| format!("the last row has no number in column {name}"))?;
    if !value.is_finite() {
        return Err(format!("{name} is {value}, which is not finite"));
    }
    Ok(value)
}

/// The kind of block of the `.dat` file, and the number of leading columns before the values.
fn block_of(field: &Field) -> (&'static str, usize, usize) {
    match field {
        Field::U => ("displacements", 1, 3),
        Field::RF => ("forces", 1, 3),
        Field::S => ("stresses", 2, 6),
        Field::NT => ("temperatures", 1, 1),
        Field::Cstr => ("contact stress", 2, 3),
        // `mode_value` and `last_row` read these. They have no block of a set.
        Field::Freq | Field::Buckle | Field::Table { .. } => ("", 0, 0),
    }
}

/// The column of a component in the values of a row.
fn column_of(q: &Quantity) -> Result<usize, String> {
    let column = match (&q.field, q.component) {
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
    let (kind, lead, width) = block_of(&q.field);
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
    use super::last_row;

    const TABLE: &str = "# Force and moment coefficients\n# magUInf : 1\n#\n\
        # Time    \tCd    \tCd(f)  \tCd(r) \tCl    \tCmPitch\n\
        1         \t0.75  \t-0.35  \t1.1   \t1.47  \t0.2    \n\
        1500      \t0.0094\t0.004  \t0.005 \t0.4448\t-0.1151\n";

    #[test]
    fn a_value_is_the_column_of_its_name_in_the_last_row() {
        assert_eq!(last_row(TABLE, "Cl"), Ok(0.4448));
        assert_eq!(last_row(TABLE, "Cd"), Ok(0.0094));
        assert_eq!(last_row(TABLE, "CmPitch"), Ok(-0.1151));
        assert!(last_row(TABLE, "Cs").unwrap_err().contains("no column Cs"));
        let cut = TABLE.replace("\t-0.1151", "");
        assert!(last_row(&cut, "Cl")
            .unwrap_err()
            .contains("5 words for 6 columns"));
        assert!(last_row("# Time Cd\n", "Cd")
            .unwrap_err()
            .contains("no row"));
        assert!(last_row("# Time Cd\n1 nan\n", "Cd")
            .unwrap_err()
            .contains("not finite"));
        assert!(last_row("# Time Cd\n1 x\n", "Cd")
            .unwrap_err()
            .contains("no number in column Cd"));
    }

    /// A watch point of preCICE has no comment line: its first line names the columns.
    #[test]
    fn a_table_with_no_comment_line_names_the_columns_in_its_first_line() {
        let table = "  Time  Coordinate0  Displacement0  Displacement1\n\
            \x20 0.0e+00   6.0e-01   0.0e+00   0.0e+00\n\
            \x20 1.6e+01   6.0e-01   2.2e-05   8.1e-04";
        assert_eq!(last_row(table, "Displacement1"), Ok(8.1e-4));
        assert!(last_row(&table[..table.find('\n').unwrap()], "Time")
            .unwrap_err()
            .contains("no row"));
    }

    /// `surfaceFieldValue` writes a vector as `(x y z)`. Thus the words of a row do not match the
    /// columns, and the reader must not take the value of a wrong column.
    #[test]
    fn a_vector_column_is_an_error() {
        let table = "# Time\tmax(U)\tmax(Ux)\n20\t(3.53 0.09 0)\t3.53\n";
        assert!(last_row(table, "max(Ux)")
            .unwrap_err()
            .contains("components"));
        let table = "# Time\tareaNormalAverage(grad(T))\n2000\t4.5308763737e+00\n";
        assert_eq!(
            last_row(table, "areaNormalAverage(grad(T))"),
            Ok(4.5308763737)
        );
    }
}
