//! Run the input of a case with `ccx`, or the OpenFOAM, GetDP, preCICE, Chrono, LAMMPS, Meep,
//! openEMS, topology and shape optimization and fit cases of a case with their programs, loops
//! and `fh fit`, and read one number per reference from the text results.

use std::path::Path;

use super::fixture::{Case, Field, Op, Quantity};
use super::run::Solve;
use crate::ccx::Ccx;
use crate::chrono::{self, Chrono};
use crate::foam::{self, Foam};
use crate::getdp::{self, GetDp};
use crate::lammps::{self, Lammps};
use crate::meep::{self, Meep};
use crate::openems::{self, OpenEms};
use crate::optimize;
use crate::precice::{self, Precice};
use crate::topology;
use crate::Solver;

/// Runs `ccx`, OpenFOAM, GetDP, preCICE, Chrono, LAMMPS, Meep, openEMS or the topology loop in a
/// scratch folder on the input of a case.
#[derive(Default)]
pub struct Subprocess {
    solver: Ccx,
    foam: Foam,
    getdp: GetDp,
    chrono: Chrono,
    meep: Meep,
    openems: OpenEms,
    precice: Precice,
    lammps: Lammps,
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

    /// Solve the OpenFOAM, GetDP, preCICE, Chrono, LAMMPS, Meep, openEMS or topology case of each
    /// check at the same time, each in its own folder in `run`, and read the value of each check in
    /// its row of its table (the last row by default).
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
                        if crate::fit::is_case(&dir) {
                            let fitted = crate::fit::run_case(&dir, &out);
                            fitted
                                .map(|()| crate::fit::WORK)
                                .map_err(|e| format!("fit {folder}: {e}"))
                        } else {
                            match crate::solver(&dir) {
                                Some(Solver::GetDp) => {
                                    let solved = self.getdp.run(&dir, &out);
                                    solved
                                        .map(|()| getdp::WORK)
                                        .map_err(|e| format!("getdp {folder}: {e}"))
                                }
                                Some(Solver::Lammps) => {
                                    let solved = self.lammps.run(&dir, &out);
                                    solved
                                        .map(|()| lammps::WORK)
                                        .map_err(|e| format!("lmp {folder}: {e}"))
                                }
                                Some(Solver::Topology) => {
                                    let cancel = crate::ccx::CancelToken::new();
                                    let solved = topology::run_with(
                                        &self.solver,
                                        &dir,
                                        &out,
                                        &cancel,
                                        &mut |_| {},
                                    );
                                    solved
                                        .map(|_| topology::WORK)
                                        .map_err(|e| format!("topology {folder}: {e}"))
                                }
                                Some(Solver::Optimize) => {
                                    let cancel = crate::ccx::CancelToken::new();
                                    let solved = optimize::run_with(
                                        &self.solver,
                                        &dir,
                                        &out,
                                        &cancel,
                                        &mut |_| {},
                                    );
                                    solved
                                        .map(|()| optimize::WORK)
                                        .map_err(|e| format!("optimize {folder}: {e}"))
                                }
                                Some(Solver::Chrono) => {
                                    let solved = self.chrono.run(&dir, &out);
                                    solved
                                        .map(|()| chrono::WORK)
                                        .map_err(|e| format!("chrono {folder}: {e}"))
                                }
                                Some(Solver::Meep) => {
                                    let solved = self.meep.run(&dir, &out);
                                    solved
                                        .map(|()| meep::WORK)
                                        .map_err(|e| format!("meep {folder}: {e}"))
                                }
                                Some(Solver::OpenEms) => {
                                    let solved = self.openems.run(&dir, &out);
                                    solved
                                        .map(|()| openems::WORK)
                                        .map_err(|e| format!("openems {folder}: {e}"))
                                }
                                Some(Solver::Grid) => {
                                    let solved = crate::grid::run(&dir, &out);
                                    solved
                                        .map(|()| crate::grid::WORK)
                                        .map_err(|e| format!("grid {folder}: {e}"))
                                }
                                Some(Solver::Precice) => {
                                    let solved = self.precice.run(&dir, &out);
                                    solved
                                        .map(|()| precice::WORK)
                                        .map_err(|e| format!("precice {folder}: {e}"))
                                }
                                _ => {
                                    let solved = self.foam.run(&dir, &out);
                                    solved
                                        .map(|()| foam::WORK)
                                        .map_err(|e| format!("openfoam {folder}: {e}"))
                                }
                            }
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
                let Field::Table { file, column, row } = &r.quantity.field else {
                    unreachable!("a case with tables has table checks only")
                };
                let path = run.join(folder).join(work).join(file);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                table_value(&text, column, *row).map_err(|e| format!("{folder}: {file}: {e}"))
            })
            .collect()
    }
}

/// Solves the grid cases of a case one after the other in this process and thread, in the run
/// folder `run`, and keeps the results there. It starts no program and no thread, so it runs in a
/// browser (`fh-quine`). A case that needs another solver is an error that names the solver.
pub struct Native {
    pub run: std::path::PathBuf,
}

impl Solve for Native {
    fn measure(&self, case: &Case, input: &Path) -> Result<Vec<f64>, String> {
        if !case.tables() {
            return Err(
                "the check reads job.inp, which needs CalculiX: the native solvers \
                        solve only grid.toml cases"
                    .into(),
            );
        }
        case.references
            .iter()
            .map(|r| {
                let folder = r.quantity.set.as_str();
                let (dir, out) = (input.join(folder), self.run.join(folder));
                match crate::solver(&dir) {
                    Some(Solver::Grid) => {}
                    Some(other) => {
                        return Err(format!(
                            "{folder} needs the solver {other:?}: the native solvers solve only \
                             grid.toml cases"
                        ))
                    }
                    None => return Err(format!("{folder} is not a case folder")),
                }
                // Each folder solves once, at its first check.
                if !out.join(crate::grid::WORK).is_dir() {
                    crate::grid::run(&dir, &out).map_err(|e| format!("grid {folder}: {e}"))?;
                }
                let Field::Table { file, column, row } = &r.quantity.field else {
                    unreachable!("a case with tables has table checks only")
                };
                let path = out.join(crate::grid::WORK).join(file);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                table_value(&text, column, *row).map_err(|e| format!("{folder}: {file}: {e}"))
            })
            .collect()
    }
}

/// The value in column `name` of the row `row` (from 1) of a table, else of its last row, for
/// example `coefficient.dat` of `forceCoeffs`. The last comment line names the columns:
/// `# Time Cd Cd(f) Cd(r) Cl ...`. A table with no comment line, for example a watch point of
/// preCICE, names the columns in its first line. Each column holds one number in each row. A mode
/// table of the grid solver has 1 row for each mode.
fn table_value(text: &str, name: &str, row: Option<usize>) -> Result<f64, String> {
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
    let mut rows = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty() && !std::ptr::eq(*l, header));
    let row: Vec<&str> = match row {
        None => rows.next_back().ok_or("no row")?,
        Some(r) => rows
            .nth(r - 1)
            .ok_or_else(|| format!("the table has no row {r}"))?,
    }
    .split_whitespace()
    .collect();
    // A vector, for example `(1 0 0)`, is 3 words in 1 column.
    if row.len() != names.len() {
        return Err(format!(
            "the row has {} words for {} columns. Write each vector field as its components \
             with the function object `components`",
            row.len(),
            names.len()
        ));
    }
    let value: f64 = row[column]
        .parse()
        .map_err(|_| format!("the row has no number in column {name}"))?;
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
        // `mode_value`, `table_value` and `rms_values` read these. They have no block of a set.
        Field::Freq | Field::Buckle | Field::ARms | Field::Table { .. } => ("", 0, 0),
    }
}

/// The column of a component in the values of a row.
fn column_of(q: &Quantity) -> Result<usize, String> {
    let column = match (&q.field, q.component) {
        (Field::U | Field::RF | Field::Cstr | Field::ARms, c @ 1..=3) => c as usize - 1,
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

/// The RMS of the acceleration of one component at each node of the set, over the frequencies of the last steady
/// state step.
fn rms_values(dat: &str, q: &Quantity) -> Result<Vec<f64>, String> {
    let column = column_of(q)?;
    let sets = crate::ccx::rms::of_dat(dat)?;
    let set = sets
        .iter()
        .find(|s| s.set.eq_ignore_ascii_case(&q.set))
        .ok_or_else(|| {
            format!(
                "no displacements of a *STEADY STATE DYNAMICS step for set `{}` in the .dat file. \
                 Add *NODE PRINT of U for it to that step",
                q.set
            )
        })?;
    Ok(set.nodes.iter().map(|(_, _, a)| a[column]).collect())
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
/// A frequency or a buckling factor comes from the last table of its step. An RMS comes from all
/// the frequencies of the last steady state step.
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
    let picked = match q.field {
        Field::ARms => rms_values(dat, q)?,
        _ => values_of(dat, q)?,
    };
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
    use super::table_value;

    const TABLE: &str = "# Force and moment coefficients\n# magUInf : 1\n#\n\
        # Time    \tCd    \tCd(f)  \tCd(r) \tCl    \tCmPitch\n\
        1         \t0.75  \t-0.35  \t1.1   \t1.47  \t0.2    \n\
        1500      \t0.0094\t0.004  \t0.005 \t0.4448\t-0.1151\n";

    /// A check with `row` reads that row, for example 1 mode of a mode table.
    #[test]
    fn a_value_with_a_row_is_in_that_row() {
        let table = "# mode frequency\n1 0.0\n2 120.5\n3 131.0\n";
        assert_eq!(table_value(table, "frequency", Some(2)), Ok(120.5));
        assert_eq!(table_value(table, "frequency", None), Ok(131.0));
        assert!(table_value(table, "frequency", Some(4))
            .unwrap_err()
            .contains("no row 4"));
    }

    #[test]
    fn a_value_is_the_column_of_its_name_in_the_last_row() {
        assert_eq!(table_value(TABLE, "Cl", None), Ok(0.4448));
        assert_eq!(table_value(TABLE, "Cd", None), Ok(0.0094));
        assert_eq!(table_value(TABLE, "CmPitch", None), Ok(-0.1151));
        assert!(table_value(TABLE, "Cs", None)
            .unwrap_err()
            .contains("no column Cs"));
        let cut = TABLE.replace("\t-0.1151", "");
        assert!(table_value(&cut, "Cl", None)
            .unwrap_err()
            .contains("5 words for 6 columns"));
        assert!(table_value("# Time Cd\n", "Cd", None)
            .unwrap_err()
            .contains("no row"));
        assert!(table_value("# Time Cd\n1 nan\n", "Cd", None)
            .unwrap_err()
            .contains("not finite"));
        assert!(table_value("# Time Cd\n1 x\n", "Cd", None)
            .unwrap_err()
            .contains("no number in column Cd"));
    }

    /// A watch point of preCICE has no comment line: its first line names the columns.
    #[test]
    fn a_table_with_no_comment_line_names_the_columns_in_its_first_line() {
        let table = "  Time  Coordinate0  Displacement0  Displacement1\n\
            \x20 0.0e+00   6.0e-01   0.0e+00   0.0e+00\n\
            \x20 1.6e+01   6.0e-01   2.2e-05   8.1e-04";
        assert_eq!(table_value(table, "Displacement1", None), Ok(8.1e-4));
        assert!(
            table_value(&table[..table.find('\n').unwrap()], "Time", None)
                .unwrap_err()
                .contains("no row")
        );
    }

    /// `surfaceFieldValue` writes a vector as `(x y z)`. Thus the words of a row do not match the
    /// columns, and the reader must not take the value of a wrong column.
    #[test]
    fn a_vector_column_is_an_error() {
        let table = "# Time\tmax(U)\tmax(Ux)\n20\t(3.53 0.09 0)\t3.53\n";
        assert!(table_value(table, "max(Ux)", None)
            .unwrap_err()
            .contains("components"));
        let table = "# Time\tareaNormalAverage(grad(T))\n2000\t4.5308763737e+00\n";
        assert_eq!(
            table_value(table, "areaNormalAverage(grad(T))", None),
            Ok(4.5308763737)
        );
    }
}
