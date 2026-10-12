//! Results files for the subprocess protocol: VTU for each step, PVD and a
//! provisional plain text summary.

use std::fs;
use std::path::Path;

use fh_inp::Model;
use fh_io::vtu::{
    write_pvd, write_vtu, Field, Grid, PvdEntry, VTK_HEXAHEDRON, VTK_QUADRATIC_TETRA,
};
use fh_solver_api::{ApiError, RunReport, PVD_FILE, SUMMARY_FILE};

use crate::element::Kind;
use crate::{solve, Solution};

/// The reference solver. Its results are unvalidated until the suite passes.
pub struct CoreSolver;

impl CoreSolver {
    /// Solve `model` and write the protocol files in `run_dir`.
    pub fn solve(&self, model: &Model, run_dir: &Path) -> Result<RunReport, ApiError> {
        let solution = solve(model).map_err(|e| ApiError(e.to_string()))?;
        write_results(model, &solution, run_dir)
    }
}

fn flat(v: &[[f64; 3]]) -> Vec<f64> {
    v.iter().flatten().copied().collect()
}

fn max_norm(v: &[[f64; 3]]) -> f64 {
    v.iter()
        .map(|a| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt())
        .fold(0.0, f64::max)
}

/// The grid without result fields: points, cells, ids and sets.
fn base_grid(model: &Model) -> Result<Grid, ApiError> {
    let index: std::collections::HashMap<u32, u32> = model
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    let mut grid = Grid {
        points: model.nodes.iter().map(|n| n.xyz).collect(),
        ..Grid::default()
    };
    for el in &model.elements {
        let kind = Kind::parse(&el.element_type)
            .ok_or_else(|| ApiError(format!("element {}: unsupported type", el.id)))?;
        grid.cell_types.push(match kind {
            Kind::C3D10 => VTK_QUADRATIC_TETRA,
            _ => VTK_HEXAHEDRON,
        });
        grid.cells.push(el.nodes.iter().map(|n| index[n]).collect());
    }
    let ids = |v: &[u32]| v.iter().map(|&i| i as i32).collect::<Vec<_>>();
    grid.point_data.push(Field::i32(
        "NODE_ID",
        ids(&model.nodes.iter().map(|n| n.id).collect::<Vec<_>>()),
    ));
    grid.cell_data.push(Field::i32(
        "ELEMENT_ID",
        ids(&model.elements.iter().map(|e| e.id).collect::<Vec<_>>()),
    ));
    for s in &model.nsets {
        let member: std::collections::HashSet<u32> = s.ids.iter().copied().collect();
        let flags = model
            .nodes
            .iter()
            .map(|n| i32::from(member.contains(&n.id)))
            .collect();
        grid.point_data
            .push(Field::i32(&format!("NSET_{}", s.name), flags));
    }
    for s in &model.elsets {
        let member: std::collections::HashSet<u32> = s.ids.iter().copied().collect();
        let flags = model
            .elements
            .iter()
            .map(|e| i32::from(member.contains(&e.id)))
            .collect();
        grid.cell_data
            .push(Field::i32(&format!("ELSET_{}", s.name), flags));
    }
    Ok(grid)
}

/// Write `step-N.vtu`, `results.pvd` and `summary.txt` in `run_dir`.
pub fn write_results(
    model: &Model,
    solution: &Solution,
    run_dir: &Path,
) -> Result<RunReport, ApiError> {
    let io = |what: &str, e: std::io::Error| ApiError(format!("{what}: {e}"));
    fs::create_dir_all(run_dir).map_err(|e| io("run folder", e))?;
    let base = base_grid(model)?;
    let mut entries = Vec::new();
    let mut lines = Vec::new();
    for (i, step) in solution.steps.iter().enumerate() {
        let mut grid = base.clone();
        grid.point_data
            .push(Field::f64("U", 3, flat(&step.displacement)));
        grid.point_data
            .push(Field::f64("RF", 3, flat(&step.reaction)));
        if step.rotation.iter().flatten().any(|v| *v != 0.0) {
            grid.point_data
                .push(Field::f64("UR", 3, flat(&step.rotation)));
        }
        grid.cell_data.push(Field::f64(
            "S",
            6,
            step.element_stress.iter().flatten().copied().collect(),
        ));
        grid.cell_data
            .push(Field::f64("MISES", 1, step.mises.clone()));
        let file = format!("step-{}.vtu", i + 1);
        let bytes = write_vtu(&grid).map_err(|e| ApiError(e.to_string()))?;
        fs::write(run_dir.join(&file), bytes).map_err(|e| io(&file, e))?;
        entries.push(PvdEntry {
            time: (i + 1) as f64,
            file,
        });
        let max_u = max_norm(&step.displacement);
        let max_mises = step.mises.iter().copied().fold(0.0, f64::max);
        let max_rf = max_norm(&step.reaction);
        lines.push(format!(
            "step {}: free_dofs={} max_u={max_u:e} max_mises={max_mises:e} max_rf={max_rf:e}",
            i + 1,
            solution.free_dofs[i],
        ));
    }
    fs::write(run_dir.join(PVD_FILE), write_pvd(&entries)).map_err(|e| io(PVD_FILE, e))?;
    // Provisional. The TOON summary schema is an open design item.
    let summary = format!(
        "nodes: {}\nelements: {}\ndofs: {}\n{}\n",
        model.nodes.len(),
        model.elements.len(),
        solution.total_dofs,
        lines.join("\n")
    );
    fs::write(run_dir.join(SUMMARY_FILE), &summary).map_err(|e| io(SUMMARY_FILE, e))?;
    Ok(RunReport {
        pvd: run_dir.join(PVD_FILE),
        summary,
    })
}
