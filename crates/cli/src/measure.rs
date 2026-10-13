//! Run `fh-solve` on a mesh and read one number per reference from the result files.

use fh_io::vtu::{read_vtu, Grid, Values};
use fh_validation::{Case, Field, Op, Quantity, Solve};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Runs the `fh-solve` binary in a scratch folder for every mesh.
pub struct Subprocess {
    exe: PathBuf,
}

impl Subprocess {
    /// The solver is `$FH_SOLVE`, or `fh-solve` next to the `fh` binary.
    pub fn locate() -> Self {
        let exe = std::env::var_os("FH_SOLVE")
            .map(PathBuf::from)
            .or_else(|| {
                let me = std::env::current_exe().ok()?;
                Some(me.with_file_name(format!("fh-solve{}", std::env::consts::EXE_SUFFIX)))
            })
            .unwrap_or_else(|| PathBuf::from("fh-solve"));
        Subprocess { exe }
    }
}

impl Solve for Subprocess {
    fn measure(&self, case: &Case, mesh: &Path) -> Result<Vec<f64>, String> {
        let text = std::fs::read_to_string(mesh).map_err(|e| e.to_string())?;
        let model = fh_inp::parse(&text).map_err(|e| e.to_string())?;
        let run = std::env::temp_dir().join(format!(
            "fh-validate-{}-{}",
            std::process::id(),
            mesh.file_stem().and_then(|s| s.to_str()).unwrap_or("mesh")
        ));
        let _ = std::fs::remove_dir_all(&run);
        // An open stdin pipe keeps the solver running: end of input means cancel.
        let mut child = Command::new(&self.exe)
            .arg(mesh)
            .arg(&run)
            // The harness reads the VTU only.
            .args(["--formats", "vtu"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", self.exe.display()))?;
        let _stdin = child.stdin.take();
        let out = child
            .wait_with_output()
            .map_err(|e| format!("cannot wait for {}: {e}", self.exe.display()))?;
        let result = if out.status.success() {
            case.references
                .iter()
                .map(|r| {
                    let step = model
                        .steps
                        .iter()
                        .position(|s| {
                            s.name
                                .as_deref()
                                .is_some_and(|n| n.eq_ignore_ascii_case(&r.quantity.step))
                        })
                        .ok_or_else(|| format!("no step `{}`", r.quantity.step))?;
                    let file = run.join(format!("step-{}.vtu", step + 1));
                    let bytes =
                        std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                    let grid = read_vtu(&bytes).map_err(|e| e.to_string())?;
                    extract(&grid, &r.quantity)
                })
                .collect()
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(format!(
                "fh-solve exited with {}: {}",
                out.status,
                err.lines().last().unwrap_or("")
            ))
        };
        let _ = std::fs::remove_dir_all(&run);
        result
    }
}

/// Reduce the values of the set to the one number of the quantity.
pub fn extract(grid: &Grid, q: &Quantity) -> Result<f64, String> {
    let (flag_name, values, width, comp) = match q.field {
        Field::U | Field::RF => {
            let name = if q.field == Field::U { "U" } else { "RF" };
            let f = grid
                .point_field(name)
                .ok_or(format!("no point field {name}"))?;
            let flag = grid
                .point_data
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(&format!("NSET_{}", q.set)))
                .ok_or(format!("no node set `{}` in the results", q.set))?;
            (
                flag,
                f,
                f.components,
                (q.component as usize).wrapping_sub(1),
            )
        }
        Field::S => {
            let f = grid.cell_field("S").ok_or("no cell field S")?;
            let flag = grid
                .cell_data
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(&format!("ELSET_{}", q.set)))
                .ok_or(format!("no element set `{}` in the results", q.set))?;
            let c = match q.component {
                11 => 0,
                22 => 1,
                33 => 2,
                12 => 3,
                23 => 4,
                13 => 5,
                n => return Err(format!("bad stress component {n}")),
            };
            (flag, f, f.components, c)
        }
    };
    if comp >= width {
        return Err(format!("component {} is out of range", q.component));
    }
    let data = values.as_f64().ok_or("result field is not float")?;
    let flags = match &flag_name.values {
        Values::I32(v) => v.clone(),
        Values::F64(v) => v.iter().map(|x| *x as i32).collect(),
    };
    let picked: Vec<f64> = flags
        .iter()
        .enumerate()
        .filter(|(_, &f)| f != 0)
        .map(|(i, _)| data[i * width + comp])
        .collect();
    if picked.is_empty() {
        return Err(format!("set `{}` is empty", q.set));
    }
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
                "set `{}` holds {} entities, op value needs 1",
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

    /// A grid of points with a field U and a node set N, from the flags.
    fn grid(u: Vec<f64>, flags: Vec<i32>) -> Grid {
        Grid {
            points: vec![[0.0; 3]; flags.len()],
            point_data: vec![
                fh_io::vtu::Field::f64("U", 3, u),
                fh_io::vtu::Field::i32("NSET_N", flags),
            ],
            ..Grid::default()
        }
    }

    fn ux(op: Op) -> Quantity {
        Quantity {
            set: "N".into(),
            field: Field::U,
            component: 1,
            step: "LOAD".into(),
            op,
        }
    }

    #[test]
    fn a_value_that_is_not_finite_is_an_error_for_every_op() {
        let g = grid(vec![f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0], vec![1, 1]);
        let passed: Vec<_> = [Op::Mean, Op::Max, Op::Min, Op::Sum]
            .into_iter()
            .filter_map(|op| extract(&g, &ux(op)).ok().map(|v| (op, v)))
            .collect();
        assert!(passed.is_empty(), "{passed:?}");
        // Finite values with a sum that overflows.
        let g = grid(vec![1e308, 0.0, 0.0, 1e308, 0.0, 0.0], vec![1, 1]);
        assert!(extract(&g, &ux(Op::Sum)).is_err());
    }
}
