//! `fh runs`: print the runs of the state folder as a plain text table, for a terminal with no
//! page, for example over SSH.
//!
//! - `fh runs` prints 1 row for each run: the run, the solver, the status, the wall time, the
//!   key result and the input.
//! - `fh runs RUN` prints the folder of the run and the path of its report. `RUN` is the run ID
//!   (`run-000012`) or its number (`12`).

use std::process::ExitCode;

use fh::daemon::{default_state_dir, run_paths, run_records};

const USAGE: &str = "usage: fh runs [RUN]\n\
fh runs prints each run of the state folder: the solver, the status, the wall time, the key \
result and the input. fh runs RUN prints the folder and the report of the run. RUN is the run ID \
(run-000012) or its number (12).";

/// The longest key result in the table. A longer one ends with `...`.
const RESULT_WIDTH: usize = 60;

/// A wall time in seconds, minutes or hours.
fn duration(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if seconds < 60.0 {
        format!("{seconds:.1} s")
    } else if s < 3600 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{} h {} min", s / 3600, s % 3600 / 60)
    }
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= RESULT_WIDTH {
        text.to_owned()
    } else {
        let start: String = text.chars().take(RESULT_WIDTH - 3).collect();
        format!("{start}...")
    }
}

/// The rows as columns of the width of the longest cell. The last column has no padding.
fn table(rows: &[[String; 6]]) -> String {
    let mut widths = [0; 6];
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let mut text = String::new();
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:w$}"))
            .collect();
        text.push_str(cells.join("  ").trim_end());
        text.push('\n');
    }
    text
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let state =
        default_state_dir().ok_or_else(|| format!("no home folder, so set FH_STATE\n{USAGE}"))?;
    match args {
        [] => {
            let records = run_records(&state);
            if records.is_empty() {
                println!("no runs in {}", state.join("runs").display());
                return Ok(ExitCode::SUCCESS);
            }
            let mut rows =
                vec![["RUN", "SOLVER", "STATUS", "TIME", "RESULT", "INPUT"].map(String::from)];
            rows.extend(records.into_iter().map(|r| {
                [
                    r.id,
                    r.solver,
                    r.status,
                    r.seconds.map_or("-".into(), duration),
                    if r.result.is_empty() {
                        "-".into()
                    } else {
                        shorten(&r.result)
                    },
                    r.source,
                ]
            }));
            print!("{}", table(&rows));
        }
        [id] if !id.starts_with('-') => {
            let (folder, report) = run_paths(&state, id)?;
            println!("folder {}", folder.display());
            match report {
                Some(report) => println!("report {}", report.display()),
                None => println!("report none"),
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::{duration, shorten, table};

    #[test]
    fn a_duration_has_the_largest_unit() {
        assert_eq!(duration(2.34), "2.3 s");
        assert_eq!(duration(125.0), "2 min 5 s");
        assert_eq!(duration(6000.0), "1 h 40 min");
    }

    #[test]
    fn the_columns_align_and_a_long_result_ends_with_dots() {
        let rows = [
            ["a", "bb", "c", "d", "e", "f"].map(String::from),
            ["aaa", "b", "c", "d", "e", ""].map(String::from),
        ];
        assert_eq!(table(&rows), "a    bb  c  d  e  f\naaa  b   c  d  e\n");
        let long = "x".repeat(100);
        assert_eq!(shorten(&long).chars().count(), 60);
        assert!(shorten(&long).ends_with("..."));
    }
}
