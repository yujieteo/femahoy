//! Run `fh-solve` on a mesh and read one number per reference from the result files.

use fh_io::vtu::{read_vtu, Grid, Values};
use fh_validation::{Case, Field, Manufactured, Op, Quantity, Solve};
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

impl Subprocess {
    /// Run `fh-solve` on `input` in a scratch folder named after `stem`. `read` gets the folder
    /// of the results. The folder is removed afterwards.
    fn run<T>(
        &self,
        input: &Path,
        stem: &str,
        read: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Result<T, String> {
        let run = std::env::temp_dir().join(format!("fh-validate-{}-{stem}", std::process::id()));
        let _ = std::fs::remove_dir_all(&run);
        // An open stdin pipe keeps the solver running: end of input means cancel.
        let mut child = Command::new(&self.exe)
            .arg(input)
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
            read(&run)
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

fn stem(mesh: &Path) -> &str {
    mesh.file_stem().and_then(|s| s.to_str()).unwrap_or("mesh")
}

fn read_step(run: &Path, step: usize) -> Result<Grid, String> {
    let file = run.join(format!("step-{}.vtu", step + 1));
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    read_vtu(&bytes).map_err(|e| e.to_string())
}

impl Solve for Subprocess {
    fn measure(&self, case: &Case, mesh: &Path) -> Result<Vec<f64>, String> {
        let text = std::fs::read_to_string(mesh).map_err(|e| e.to_string())?;
        let model = fh_inp::parse_file(&mesh.display().to_string(), &text)
            .map_err(|e| fh_inp::join_errors(&e))?;
        self.run(mesh, stem(mesh), |run| {
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
                    extract(&read_step(run, step)?, &r.quantity)
                })
                .collect()
        })
    }

    fn field_error(&self, case: &Case, mesh: &Path) -> Result<f64, String> {
        let m = case
            .manufactured
            .as_ref()
            .ok_or("the case has no manufactured solution")?;
        let text = std::fs::read_to_string(mesh).map_err(|e| e.to_string())?;
        let model = fh_inp::parse_file(&mesh.display().to_string(), &text)
            .map_err(|e| fh_inp::join_errors(&e))?;
        let boundary = model
            .nset(&m.boundary_set)
            .ok_or_else(|| format!("no node set `{}`", m.boundary_set))?;
        // The mesh has no step. The harness adds one with the values of the exact field.
        let mut input = text;
        input.push_str("\n*STEP\n*STATIC\n*BOUNDARY\n");
        for id in &boundary.ids {
            let node = model
                .nodes
                .iter()
                .find(|n| n.id == *id)
                .ok_or_else(|| format!("node {id} of `{}` is not defined", m.boundary_set))?;
            for (d, e) in m.exact.iter().enumerate() {
                input.push_str(&format!("{id}, {0}, {0}, {1:e}\n", d + 1, e.eval(node.xyz)));
            }
        }
        input.push_str("*END STEP\n");
        let dir = std::env::temp_dir().join(format!("fh-validate-{}-in", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("{}.inp", stem(mesh)));
        std::fs::write(&file, input).map_err(|e| e.to_string())?;
        let result = self.run(&file, stem(mesh), |run| rms_error(&read_step(run, 0)?, m));
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
        result
    }
}

/// The root mean square, over all points, of the distance between `U` and the exact field.
fn rms_error(grid: &Grid, m: &Manufactured) -> Result<f64, String> {
    let u = grid
        .point_field("U")
        .and_then(|f| f.as_f64().filter(|_| f.components == 3))
        .ok_or("no point field U with 3 components")?;
    if u.len() != 3 * grid.points.len() || grid.points.is_empty() {
        return Err(format!(
            "U has {} values for {} points",
            u.len(),
            grid.points.len()
        ));
    }
    let sum: f64 = grid
        .points
        .iter()
        .zip(u.chunks(3))
        .map(|(p, got)| {
            (0..3)
                .map(|d| (got[d] - m.exact[d].eval(*p)).powi(2))
                .sum::<f64>()
        })
        .sum();
    let rms = (sum / grid.points.len() as f64).sqrt();
    if rms.is_finite() {
        Ok(rms)
    } else {
        Err("the error of U is not finite".into())
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
    // The data comes from a solver, so its sizes are checked before any access.
    let entities = match q.field {
        Field::U | Field::RF => grid.points.len(),
        Field::S => grid.cells.len(),
    };
    if flags.len() != entities || entities.checked_mul(width) != Some(data.len()) {
        return Err(format!(
            "set `{}` has {} flags and the result field has {} values, for {entities} entities with {width} components",
            q.set,
            flags.len(),
            data.len()
        ));
    }
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

    #[test]
    fn a_field_or_a_set_with_the_wrong_length_is_an_error_not_a_panic() {
        let u = vec![1.0, 0.0, 0.0, 2.0, 0.0, 0.0];
        // A set with more flags than points, and one with fewer.
        for flags in [vec![1, 1, 1], vec![1]] {
            let mut g = grid(u.clone(), vec![1, 1]);
            g.point_data[1] = fh_io::vtu::Field::i32("NSET_N", flags.clone());
            let r = std::panic::catch_unwind(|| extract(&g, &ux(Op::Max)));
            assert!(matches!(r, Ok(Err(_))), "{flags:?}: {r:?}");
        }
        // U with 6 components for 2 points: 6 values, not 12.
        let mut g = grid(u, vec![1, 1]);
        g.point_data[0].components = 6;
        let r = std::panic::catch_unwind(|| extract(&g, &ux(Op::Max)));
        assert!(matches!(r, Ok(Err(_))), "{r:?}");
    }
}
