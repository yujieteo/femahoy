//! LAMMPS as the particle solver of femahoy (SPH, DEM), in the process of `fh`.
//!
//! A LAMMPS case is a folder with the input script `case.lmp` and the files that it reads.
//! [`Lammps::run`] copies the files of the folder to `<run-dir>/lammps` and starts there the
//! user's LAMMPS (`$FH_LAMMPS`, else `lmp` on PATH): `lmp -in case.lmp -log none`. The output of
//! LAMMPS goes to `<run-dir>/lammps/log.lmp`. The script writes its text results (for example
//! with `print ... file`) to the files that it names.
//!
//! Then the run writes each snapshot of the text dump files (`*.dump`, from `dump ... custom`)
//! to `step-N.vtu` and `results.pvd`. Each particle is a cube at its position in the snapshot,
//! and each column other than `id`, `x`, `y` and `z` is a cell field. The side of the cubes is
//! the smallest distance between 2 particles of the first snapshot. Each snapshot must have the
//! particles of the first snapshot.
//!
//! LAMMPS is GPL, so femahoy only starts it as a separate program. A run has the limits of a
//! run of `ccx` (see [`crate::ccx`]).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use fh_core::vtu::{Field, Grid};

use crate::ccx::{
    default_memory_limit, log_tail, prepare_run_dir, supervise, write_frames, CancelToken, Program,
};
use crate::getdp::copy_case;

/// The folder of the LAMMPS files in a run folder.
pub const WORK: &str = "lammps";

/// True when `dir` is a LAMMPS case: it has `case.lmp`.
pub fn is_case(dir: &Path) -> bool {
    dir.join("case.lmp").is_file()
}

/// Why LAMMPS failed, from its exit status and its output `log`. LAMMPS starts each error line
/// with `ERROR` and then names the line of the script that failed.
fn failure(status: ExitStatus, log: &str, log_file: &Path) -> String {
    let see = format!("The full output is in {}", log_file.display());
    let errors: Vec<&str> = log
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("ERROR") || l.starts_with("Last input line"))
        .collect();
    if errors.is_empty() {
        return format!(
            "lmp failed ({status}). {see}. Its last lines: {}",
            log_tail(log)
        );
    }
    format!(
        "lmp failed ({status}): {}. Correct case.lmp as the message tells. {see}.",
        errors.join(" | ")
    )
}

/// One snapshot of a text dump: its time, the names of its columns and its rows.
#[derive(Debug)]
struct Snapshot {
    time: f64,
    columns: Vec<String>,
    rows: Vec<Vec<f64>>,
}

/// Read the snapshots of a text dump of the `custom` style. A snapshot is the lines `ITEM: TIME`
/// (with `dump_modify time yes`), `ITEM: TIMESTEP`, `ITEM: NUMBER OF ATOMS` and `ITEM: ATOMS`
/// with the names of the columns, each with its values on the next lines. The reader skips the
/// other items, for example `ITEM: BOX BOUNDS`. The time is the step when the dump has no time.
fn read_dump(text: &str) -> Result<Vec<Snapshot>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut snapshots = Vec::new();
    let (mut time, mut step, mut count) = (None, None, None);
    let mut i = 0;
    while i < lines.len() {
        let Some(item) = lines[i].strip_prefix("ITEM:").map(str::trim) else {
            i += 1;
            continue;
        };
        let at = i + 1;
        i += 1;
        let mut number = || -> Result<f64, String> {
            let text = lines.get(i).map(|l| l.trim()).unwrap_or_default();
            i += 1;
            text.parse()
                .map_err(|_| format!("line {}: {text:?} is where a number must be", at + 1))
        };
        match item {
            "TIME" => time = Some(number()?),
            "TIMESTEP" => step = Some(number()?),
            "NUMBER OF ATOMS" => count = Some(number()? as usize),
            _ => {
                let Some(names) = item.strip_prefix("ATOMS") else {
                    continue;
                };
                let n = count.take().ok_or_else(|| {
                    format!("line {at}: ITEM: ATOMS comes before ITEM: NUMBER OF ATOMS")
                })?;
                let columns: Vec<String> = names.split_whitespace().map(str::to_owned).collect();
                let mut rows = Vec::with_capacity(n);
                for (k, line) in lines.iter().enumerate().skip(i).take(n) {
                    let row: Vec<f64> = line
                        .split_whitespace()
                        .map(str::parse)
                        .collect::<Result<_, _>>()
                        .map_err(|_| format!("line {}: a word is not a number", k + 1))?;
                    if row.len() != columns.len() {
                        return Err(format!(
                            "line {}: {} numbers for {} columns",
                            k + 1,
                            row.len(),
                            columns.len()
                        ));
                    }
                    rows.push(row);
                }
                if rows.len() != n {
                    return Err(format!("line {at}: the snapshot has less than {n} rows"));
                }
                i += n;
                let time = time.take().or(step.take()).unwrap_or(0.0);
                snapshots.push(Snapshot {
                    time,
                    columns,
                    rows,
                });
            }
        }
    }
    Ok(snapshots)
}

/// The smallest distance between 2 points that are not at the same position.
fn spacing(points: &[[f64; 3]]) -> f64 {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut best = f64::INFINITY;
    for (k, p) in sorted.iter().enumerate() {
        for q in &sorted[k + 1..] {
            if q[0] - p[0] >= best {
                break;
            }
            let d = (0..3).map(|c| (q[c] - p[c]).powi(2)).sum::<f64>().sqrt();
            if d > 0.0 {
                best = best.min(d);
            }
        }
    }
    best
}

/// The corners of a cube of side 2 about the origin, in the order of a VTK hexahedron.
const CORNERS: [[f64; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

/// The frames of the snapshots: a cube for each particle, at its position in the snapshot, and
/// a cell field for each column other than `id`, `x`, `y` and `z`. The rows of each snapshot
/// are in the order of `id`.
fn frames(mut snapshots: Vec<Snapshot>) -> Result<Vec<(f64, Grid)>, String> {
    let Some(first) = snapshots.first() else {
        return Ok(Vec::new());
    };
    let columns = first.columns.clone();
    let find = |name: &str| columns.iter().position(|c| c == name);
    let (Some(id), Some(x), Some(y), Some(z)) = (find("id"), find("x"), find("y"), find("z"))
    else {
        return Err(format!(
            "the dump has the columns {}. It needs id, x, y and z",
            columns.join(" ")
        ));
    };
    for s in &mut snapshots {
        if s.columns != columns {
            return Err(format!(
                "the snapshot at time {} has other columns than the first",
                s.time
            ));
        }
        s.rows.sort_by(|a, b| a[id].total_cmp(&b[id]));
    }
    let ids: Vec<f64> = snapshots[0].rows.iter().map(|r| r[id]).collect();
    let position = |r: &Vec<f64>| [r[x], r[y], r[z]];
    let half = spacing(&snapshots[0].rows.iter().map(position).collect::<Vec<_>>()) / 2.0;
    if !half.is_finite() {
        return Err("the first snapshot has less than 2 particles at different positions".into());
    }
    let fields: Vec<usize> = (0..columns.len())
        .filter(|c| ![id, x, y, z].contains(c))
        .collect();
    snapshots
        .iter()
        .map(|s| {
            if !s.rows.iter().map(|r| r[id]).eq(ids.iter().copied()) {
                return Err(format!(
                    "the snapshot at time {} has other particles than the first. Dump 1 group \
                     to 1 file, and keep each particle in the box",
                    s.time
                ));
            }
            let mut grid = Grid::default();
            for (k, row) in s.rows.iter().enumerate() {
                let p = position(row);
                grid.points
                    .extend(CORNERS.map(|corner| std::array::from_fn(|c| p[c] + half * corner[c])));
                grid.cells.push((8 * k as u32..8 * k as u32 + 8).collect());
                grid.cell_types.push(12);
            }
            for &c in &fields {
                let values = s.rows.iter().map(|r| r[c]).collect();
                grid.cell_data.push(Field::f64(&columns[c], 1, values));
            }
            Ok((s.time, grid))
        })
        .collect()
}

/// The `lmp` program with the limits of its runs.
#[derive(Debug, Clone)]
pub struct Lammps {
    /// `$FH_LAMMPS`, else `lmp` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Limit for the process group of LAMMPS in bytes, as for `ccx`. `u64::MAX` means no limit.
    pub memory_limit: u64,
}

impl Default for Lammps {
    fn default() -> Self {
        Self::new()
    }
}

impl Lammps {
    /// `lmp` from `$FH_LAMMPS`, else `lmp` on PATH. It has no time limit. The memory limit is
    /// 85 % of the machine RAM.
    pub fn new() -> Self {
        Lammps {
            exe: std::env::var_os("FH_LAMMPS").map_or_else(|| PathBuf::from("lmp"), PathBuf::from),
            timeout: None,
            memory_limit: default_memory_limit(),
        }
    }

    /// Solve the case `case` in the folder `run_dir`. The run clears the folder first, if a run
    /// made the folder earlier.
    pub fn run(&self, case: &Path, run_dir: &Path) -> Result<(), String> {
        self.run_with(case, run_dir, &CancelToken::new(), &mut |_| {})
    }

    /// Solve, and stop when `cancel` is set. `on_program` gets the name of the program that the
    /// run starts.
    pub fn run_with(
        &self,
        case: &Path,
        run_dir: &Path,
        cancel: &CancelToken,
        on_program: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        let start = Instant::now();
        let case =
            fs::canonicalize(case).map_err(|e| format!("{}: case folder: {e}", case.display()))?;
        if !is_case(&case) {
            return Err(format!(
                "{} is not a LAMMPS case: it has no case.lmp",
                case.display()
            ));
        }
        let run_dir = prepare_run_dir(run_dir, &case)?;
        let work = run_dir.join(WORK);
        copy_case(&case, &work, false)?;
        let program = Program {
            name: "lmp",
            install: "Install LAMMPS (README.md) and put lmp on PATH, or set FH_LAMMPS",
        };
        on_program(program.name);
        let log_file = work.join("log.lmp");
        let folder = |e: std::io::Error| format!("run folder: {e}");
        let log = fs::File::create(&log_file).map_err(folder)?;
        let mut cmd = Command::new(&self.exe);
        cmd.args(["-in", "case.lmp", "-log", "none"])
            .current_dir(&work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(folder)?)
            .stderr(log);
        let limits = (start, self.timeout, self.memory_limit);
        let status = supervise(&program, &mut cmd, limits, cancel, &mut || {})?;
        if !status.success() {
            let log = fs::read_to_string(&log_file).unwrap_or_default();
            return Err(failure(status, &log, &log_file));
        }
        let mut files: Vec<PathBuf> = fs::read_dir(&work)
            .map_err(|e| format!("{}: {e}", work.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "dump"))
            .collect();
        files.sort();
        let mut snapshots = Vec::new();
        for file in &files {
            let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
            snapshots.extend(read_dump(&text).map_err(|e| format!("{}: {e}", file.display()))?);
        }
        write_frames(
            &run_dir,
            "unspecified",
            false,
            frames(snapshots)?.into_iter(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2 snapshots of 3 particles 0.5 apart in 2D, out of the order of `id` in the second.
    const DUMP: &str = "ITEM: TIME\n0\nITEM: TIMESTEP\n0\nITEM: NUMBER OF ATOMS\n3\n\
        ITEM: BOX BOUNDS ff ff pp\n0 2\n0 2\n-0.1 0.1\nITEM: ATOMS id x y z p\n\
        1 0 0 0 10\n2 0.5 0 0 20\n3 0 1 0 30\n\
        ITEM: TIME\n0.25\nITEM: TIMESTEP\n100\nITEM: NUMBER OF ATOMS\n3\n\
        ITEM: BOX BOUNDS ff ff pp\n0 2\n0 2\n-0.1 0.1\nITEM: ATOMS id x y z p\n\
        3 0 1.5 0 33\n1 1 0 0 11\n2 1.5 0 0 22\n";

    #[test]
    fn each_snapshot_is_a_frame_of_cubes_at_the_particles() {
        let frames = frames(read_dump(DUMP).unwrap()).unwrap();
        assert_eq!(frames.len(), 2);
        let (time, grid) = &frames[1];
        assert_eq!(*time, 0.25);
        assert_eq!(grid.cells.len(), 3);
        assert_eq!(grid.cell_types, vec![12; 3]);
        assert_eq!(grid.cells[2], (16..24).collect::<Vec<u32>>());
        // The side is the smallest distance in the first snapshot, 0.5. Particle 3 is the last.
        assert_eq!(grid.points[16], [-0.25, 1.25, -0.25]);
        assert_eq!(grid.points[22], [0.25, 1.75, 0.25]);
        assert!(grid.point_data.is_empty());
        assert_eq!(
            grid.cell_field("p").unwrap().as_f64(),
            Some(&[11.0, 22.0, 33.0][..])
        );
        // The time is the step when the dump has no time.
        let no_time = DUMP.replace("ITEM: TIME\n0.25\n", "");
        assert_eq!(frames_of(&no_time)[1].0, 100.0);
    }

    fn frames_of(text: &str) -> Vec<(f64, Grid)> {
        frames(read_dump(text).unwrap()).unwrap()
    }

    #[test]
    fn a_bad_dump_is_an_error() {
        let short = DUMP.replace("2 1.5 0 0 22\n", "");
        let err = read_dump(&short).unwrap_err();
        assert!(
            err.contains("line 25: the snapshot has less than 3 rows"),
            "{err}"
        );
        let word = DUMP.replacen("20\n", "x\n", 1);
        assert!(read_dump(&word).unwrap_err().contains("not a number"));
        let no_id = DUMP.replace("ITEM: ATOMS id", "ITEM: ATOMS n");
        let err = frames(read_dump(&no_id).unwrap()).unwrap_err();
        assert!(err.contains("It needs id, x, y and z"), "{err}");
        let lost = DUMP.replace("3 0 1.5 0 33", "4 0 1.5 0 33");
        let err = frames(read_dump(&lost).unwrap()).unwrap_err();
        assert!(err.contains("time 0.25 has other particles"), "{err}");
    }

    #[test]
    fn a_failure_shows_the_error_lines() {
        let status = Command::new("/bin/sh")
            .args(["-c", "exit 1"])
            .status()
            .unwrap();
        let log = "LAMMPS (30 Sep 2026)\nERROR: Unknown command: foo (../input.cpp:314)\n\
            Last input line: foo\n";
        let text = failure(status, log, Path::new("log.lmp"));
        assert!(
            text.contains(
                "lmp failed (exit status: 1): ERROR: Unknown command: foo (../input.cpp:314) \
                 | Last input line: foo."
            ),
            "{text}"
        );
        let plain = failure(status, "Total wall time: 0:00:01\n", Path::new("log.lmp"));
        assert!(plain.contains("Its last lines: Total wall time"), "{plain}");
    }
}
