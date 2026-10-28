//! DC current conduction with its Joule heat, and electrostatics with the capacitance matrix,
//! on the meshes of `heat`: the box grid with 8-node hex elements, or the 2.5D tetrahedral mesh
//! with 4-node tetrahedra.
//!
//! - Conduction: div(sigma grad V) = 0 in the parts, with the electric conductivity sigma. The
//!   heat equation of `heat` with other names: the conductivity is sigma, the temperature is the
//!   potential V and the heat flux is the current density J = -sigma grad V.
//! - Electrostatics: div(eps grad V) = 0 with eps = eps0 eps_r. The current is the
//!   displacement D = -eps grad V, and the charge takes the place of the current.
//!
//! A terminal is a face set with 1 potential: a fixed voltage, or a fixed total current (a
//! fixed charge) at an unknown voltage. The solve is a superposition. The terminal points are
//! fixed, and solve j has terminal j at 1 V and the others at 0 V. The current of terminal i in
//! solve j is the residual of the rows of its points (as in `line`): the conductance matrix G_ij
//! (or the Maxwell capacitance matrix C_ij), with I = G V. The voltages of the current terminals
//! then solve G_cc V_c = I_c - G_cv V_v, so each current terminal is 1 shared unknown, and a
//! floating conductor is a terminal with 0 charge. A model needs 1 voltage terminal or more: it
//! sets the reference of the potential.
//!
//! The Joule heat of cell e is u_e^T K_e u_e, with its element matrix K_e and its point voltages
//! u_e: the sum over the cells is exactly sum_i V_i I_i. Each point of the cell gets an equal
//! share, the lumped load of a uniform source on a linear element. With heat loads, a steady heat
//! solve of the same mesh takes the Joule heat as a source. A material with a temperature
//! coefficient alpha has the resistivity rho_0 (1 + alpha (T - T_ref)), with T the mean of the
//! points of each cell. Then the conduction and the heat solves repeat (Picard iterations) until
//! the Joule power changes by less than `TOL`. A current drive with alpha > 0 can have no steady
//! state (thermal runaway): the solve stops after `MAX_ITER` and refuses.
//!
//! Sources: T. J. R. Hughes, The Finite Element Method, Prentice-Hall 1987, chapter 1 (the
//! steady conduction equation and its weak form, also for the electric potential); P. P.
//! Silvester and R. L. Ferrari, Finite Elements for Electrical Engineers, 3rd ed., Cambridge
//! 1996 (the electrostatic potential, and the charge and the capacitance from the residual);
//! F. P. Incropera, D. P. DeWitt, T. L. Bergman and A. S. Lavine, Fundamentals of Heat and Mass
//! Transfer, 6th ed., Wiley 2007, section 3.5 (the heat generation of an electric current in a
//! wall); R. Holm, Electric Contacts, Springer 1967, https://doi.org/10.1007/978-3-662-06688-1
//! (the constriction resistance of a circular spot, the check of a validation case).

use crate::complex::{Matrix, C64};
use crate::grid::{Part, Spacing};
use crate::heat::{self, FaceSet, Load, Mesh, Probe};
use crate::solve::{Crossover, Method, Solver, STEADY_DIRECT_LIMIT};

/// The permittivity of vacuum (F/m), CODATA 2018.
pub const EPS0: f64 = 8.8541878128e-12;

/// The Picard iterations stop when the Joule power changes by less than this fraction.
const TOL: f64 = 1e-6;

/// The most Picard iterations. Reason: a coupling that converges takes 2 to 10; one that grows
/// without a limit (thermal runaway) must stop with a message.
const MAX_ITER: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// DC current conduction, with the electric conductivity.
    Conduction,
    /// Electrostatics, with the permittivity.
    Electrostatic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// The electric conductivity (S/m) or the relative permittivity, along x, y and z.
    pub value: [f64; 3],
    /// The temperature coefficient of the resistivity (1/K), 0 for a constant conductivity.
    pub coefficient: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drive {
    /// A fixed voltage (V).
    Voltage(f64),
    /// A fixed total current into the body (A), or a charge (C) in electrostatics.
    Current(f64),
}

/// A face set at 1 potential.
#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub faces: usize,
    pub drive: Drive,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    pub spacing: Spacing,
    /// The extrusion axis of the parts with a footprint (0 x, 1 y, 2 z).
    pub axis: usize,
    pub faces: Vec<FaceSet>,
    pub terminals: Vec<Terminal>,
    pub kind: Kind,
    /// The unit of length in m: 1 for SI-m, 1e-3 for SI-mm. The unit of power of the heat solve
    /// is 1e-3 W in SI-mm too.
    pub scale: f64,
    /// The steady heat solve of the same parts with the Joule heat as a source (conduction).
    pub heat: Option<Box<heat::Model>>,
    /// The temperature where each material has its conductivity `value`.
    pub reference: f64,
    pub probes: Vec<Probe>,
    pub method: Option<Method>,
}

/// What a solve tells while it runs.
pub trait Events {
    /// True stops the solve.
    fn cancelled(&self) -> bool {
        false
    }
    /// A line of progress for the person.
    fn progress(&mut self, text: &str);
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub mesh: Mesh,
    /// The potential of each point (V).
    pub potential: Vec<f64>,
    /// The current density J (A/m2), or the electric field E (V/m), at the centre of each cell.
    pub field: Vec<[f64; 3]>,
    /// The Joule power density of each cell (W/m3), 0 in electrostatics.
    pub joule: Vec<f64>,
    /// The temperature of each point, with heat loads.
    pub temperature: Option<Vec<f64>>,
    /// The voltage and the current (or the charge) of each terminal.
    pub terminals: Vec<(f64, f64)>,
    /// The conductance matrix (S) or the capacitance matrix (F) of the terminals, row-major:
    /// I = M V.
    pub matrix: Vec<f64>,
    /// The potential and the temperature (or NaN) at each probe.
    pub probes: Vec<(f64, f64)>,
    /// The Picard iterations, 1 with no temperature coefficient.
    pub iterations: usize,
    pub unknowns: usize,
    pub method: Method,
    pub warnings: Vec<String>,
}

impl Solution {
    /// The Joule power of the model (W): sum_i V_i I_i.
    pub fn power(&self) -> f64 {
        self.terminals.iter().map(|(v, i)| v * i).sum()
    }
}

/// Solve the model.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Solution, String> {
    check(model)?;
    // The mesh of the heat model, with the material values as its conductivity. Each terminal is
    // a fixed temperature, so its face set must select a face.
    let shape = heat::Model {
        parts: model.parts.clone(),
        materials: model
            .materials
            .iter()
            .map(|m| heat::Material {
                name: m.name.clone(),
                conductivity: m.value,
                capacity: 0.0,
                conductivity_table: None,
                capacity_table: None,
            })
            .collect(),
        spacing: model.spacing,
        axis: model.axis,
        symmetry: [false; 3],
        faces: model.faces.clone(),
        loads: model
            .terminals
            .iter()
            .map(|t| Load::Temperature {
                faces: t.faces,
                value: 0.0,
            })
            .collect(),
        step: heat::Step::Steady,
        probes: model.probes.clone(),
        method: model.method,
    };
    let d = heat::discrete(&shape, heat::LAPTOP)?;
    events.progress(&d.line);
    let mesh = &d.mesh;
    let n = mesh.points.len();
    let m = model.terminals.len();
    // The terminal of each point.
    let mut owner: Vec<Option<usize>> = vec![None; n];
    for (t, terminal) in model.terminals.iter().enumerate() {
        for face in &d.faces[terminal.faces] {
            for &v in face.nodes() {
                match owner[v as usize] {
                    Some(o) if o != t => {
                        return Err(format!(
                            "the terminals {} and {} share a point: give them face sets that do \
                             not touch",
                            model.faces[model.terminals[o].faces].name,
                            model.faces[terminal.faces].name
                        ))
                    }
                    _ => owner[v as usize] = Some(t),
                }
            }
        }
    }
    let held: Vec<bool> = owner
        .iter()
        .map(|o| o.is_some_and(|t| matches!(model.terminals[t].drive, Drive::Voltage(_))))
        .collect();
    if let Some(p) = heat::loose_part(mesh, &held) {
        return Err(format!(
            "part {} has no voltage on it or on a part that it touches, so its potential has no \
             unique value: add a voltage on a face of it, or remove the part",
            model.parts[p].name
        ));
    }
    let free: Vec<usize> = (0..n).filter(|&i| owner[i].is_none()).collect();
    let per = mesh.kind.nodes();
    let base: Vec<[f64; 3]> = (0..mesh.len())
        .map(|e| {
            let v = model.materials[model.parts[mesh.part[e] as usize].material].value;
            let f = match model.kind {
                Kind::Conduction => model.scale,
                Kind::Electrostatic => EPS0 * model.scale,
            };
            v.map(|v| v * f)
        })
        .collect();
    let alpha: Vec<f64> = (0..mesh.len())
        .map(|e| model.materials[model.parts[mesh.part[e] as usize].material].coefficient)
        .collect();
    let coupled = model.heat.is_some() && alpha.iter().any(|&a| a != 0.0);
    let mut c = base.clone();
    let mut temperature: Option<Vec<f64>> = None;
    let mut warnings = d.warnings.clone();
    let mut last_power = f64::NAN;
    for iteration in 1..=MAX_ITER {
        if events.cancelled() {
            return Err("cancelled".into());
        }
        let mut k = crate::sparse::Csr::pattern(n, (0..mesh.len()).map(|e| mesh.cell(e)));
        let elements: Vec<Vec<f64>> = (0..mesh.len()).map(|e| (d.element)(e, c[e])).collect();
        for (e, ke) in elements.iter().enumerate() {
            k.add_element(mesh.cell(e), ke);
        }
        let solver = Solver::new(
            k.submatrix(&free),
            (d.unknowns)(&free),
            model.method,
            Crossover::Unknowns(STEADY_DIRECT_LIMIT),
            None,
        )?;
        let method = solver.method();
        if iteration == 1 {
            events.progress(&format!(
                "{} unknowns, {} solver, {m} terminal solves",
                free.len(),
                heat::name(method)
            ));
        }
        // The potential of each unit solve, and the matrix from the residuals.
        let mut unit = Vec::with_capacity(m);
        let mut matrix = vec![0.0; m * m];
        let mut kv = vec![0.0; n];
        for j in 0..m {
            let mut v: Vec<f64> = owner.iter().map(|o| f64::from(*o == Some(j))).collect();
            let rhs = heat::reduced_rhs(&k, &vec![0.0; n], &v, &free, None);
            let mut x = vec![0.0; free.len()];
            solver.solve(&rhs, &mut x, &|| events.cancelled())?;
            for (r, &i) in free.iter().enumerate() {
                v[i] = x[r];
            }
            k.mul(&v, &mut kv);
            for i in 0..n {
                if let Some(t) = owner[i] {
                    matrix[t * m + j] += kv[i];
                }
            }
            unit.push(v);
        }
        let volts = voltages(model, &matrix)?;
        let potential: Vec<f64> = (0..n)
            .map(|i| (0..m).map(|j| volts[j] * unit[j][i]).sum())
            .collect();
        let terminals: Vec<(f64, f64)> = (0..m)
            .map(|i| {
                let current = (0..m).map(|j| matrix[i * m + j] * volts[j]).sum();
                (volts[i], current)
            })
            .collect();
        // The Joule heat of each cell (W).
        let heat_of: Vec<f64> = match model.kind {
            Kind::Electrostatic => vec![0.0; mesh.len()],
            Kind::Conduction => elements
                .iter()
                .enumerate()
                .map(|(e, ke)| {
                    let u: Vec<f64> = mesh
                        .cell(e)
                        .iter()
                        .map(|&i| potential[i as usize])
                        .collect();
                    (0..per)
                        .map(|a| u[a] * (0..per).map(|b| ke[a * per + b] * u[b]).sum::<f64>())
                        .sum()
                })
                .collect(),
        };
        let power: f64 = heat_of.iter().sum();
        if let Some(h) = &model.heat {
            let mut source = vec![0.0; n];
            for (e, q) in heat_of.iter().enumerate() {
                for &i in mesh.cell(e) {
                    // W to the unit of power of the case.
                    source[i as usize] += q / model.scale / per as f64;
                }
            }
            let mut run = HeatRun {
                outer: events,
                temperature: None,
            };
            let summary = heat::solve_with_source(h, &source, &mut run)?;
            let t = run
                .temperature
                .ok_or("the heat solve gave no temperature")?;
            if t.len() != n {
                return Err("the heat model has other parts or another mesh".into());
            }
            if !coupled || iteration == 1 {
                warnings.extend(summary.warnings.iter().map(|w| format!("heat: {w}")));
            }
            temperature = Some(t);
        }
        let converged = !coupled || (power - last_power).abs() <= TOL * power.abs();
        if coupled {
            events.progress(&format!("iteration {iteration}: Joule power {power:.6e} W"));
        }
        if converged {
            let gradient = (d.gradient)(&potential);
            let field = gradient
                .iter()
                .zip(&c)
                .map(|(g, c)| match model.kind {
                    Kind::Conduction => std::array::from_fn(|a| -c[a] * g[a] / model.scale.powi(2)),
                    Kind::Electrostatic => g.map(|g| -g / model.scale),
                })
                .collect();
            let joule = heat_of
                .iter()
                .zip(&d.volume)
                .map(|(q, v)| q / (v * model.scale.powi(3)))
                .collect();
            let probes = d
                .probes
                .iter()
                .map(|w| {
                    let at = |f: &[f64]| w.iter().map(|&(i, s)| s * f[i as usize]).sum::<f64>();
                    (at(&potential), temperature.as_deref().map_or(f64::NAN, at))
                })
                .collect();
            return Ok(Solution {
                mesh: d.mesh.clone(),
                potential,
                field,
                joule,
                temperature,
                terminals,
                matrix,
                probes,
                iterations: iteration,
                unknowns: free.len(),
                method,
                warnings,
            });
        }
        last_power = power;
        let t = temperature
            .as_ref()
            .expect("a coupled model has a heat solve");
        for e in 0..mesh.len() {
            let mean = mesh.cell(e).iter().map(|&i| t[i as usize]).sum::<f64>() / per as f64;
            let factor = 1.0 + alpha[e] * (mean - model.reference);
            if !(factor > 0.0) {
                return Err(format!(
                    "part {}: at {mean:.4e} the temperature coefficient gives a resistivity of 0 \
                     or less: the model leaves the range of its linear resistivity",
                    model.parts[mesh.part[e] as usize].name
                ));
            }
            c[e] = base[e].map(|v| v / factor);
        }
    }
    Err(format!(
        "the current and the temperature do not converge in {MAX_ITER} iterations: the Joule \
         power grows with the temperature faster than the heat leaves (thermal runaway). Use a \
         voltage drive, a smaller current, or more cooling"
    ))
}

/// The voltage of each terminal: the fixed ones, and those of the current terminals from
/// G_cc V_c = I_c - G_cv V_v.
fn voltages(model: &Model, g: &[f64]) -> Result<Vec<f64>, String> {
    let m = model.terminals.len();
    let mut volts = vec![0.0; m];
    let current: Vec<usize> = (0..m)
        .filter(|&i| matches!(model.terminals[i].drive, Drive::Current(_)))
        .collect();
    for (i, t) in model.terminals.iter().enumerate() {
        if let Drive::Voltage(v) = t.drive {
            volts[i] = v;
        }
    }
    if current.is_empty() {
        return Ok(volts);
    }
    let c = current.len();
    let mut a = Matrix::zeros(c);
    let mut b = Matrix::zeros(c);
    for (r, &i) in current.iter().enumerate() {
        let Drive::Current(target) = model.terminals[i].drive else {
            unreachable!("a current terminal")
        };
        let rest: f64 = (0..m).map(|j| g[i * m + j] * volts[j]).sum();
        b.set(r, 0, C64::new(target - rest, 0.0));
        for (s, &j) in current.iter().enumerate() {
            a.set(r, s, C64::new(g[i * m + j], 0.0));
        }
    }
    let x = a.solve(&b)?;
    for (r, &i) in current.iter().enumerate() {
        volts[i] = x.at(r, 0).re;
    }
    Ok(volts)
}

/// The checks of the model that the mesh does not need.
fn check(model: &Model) -> Result<(), String> {
    for m in &model.materials {
        if m.value.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
            return Err(format!(
                "material {}: the {} must be above 0",
                m.name,
                match model.kind {
                    Kind::Conduction => "electric conductivity",
                    Kind::Electrostatic => "relative permittivity",
                }
            ));
        }
        if !m.coefficient.is_finite() {
            return Err(format!(
                "material {}: the temperature coefficient must be finite",
                m.name
            ));
        }
    }
    if !model
        .terminals
        .iter()
        .any(|t| matches!(t.drive, Drive::Voltage(_)))
    {
        return Err(
            "the model needs a voltage on 1 face set or more: it sets the reference of the \
             potential"
                .into(),
        );
    }
    for t in &model.terminals {
        let (Drive::Voltage(v) | Drive::Current(v)) = t.drive;
        if !v.is_finite() {
            return Err("each voltage and current must be finite".into());
        }
        if model
            .terminals
            .iter()
            .filter(|o| o.faces == t.faces)
            .count()
            > 1
        {
            return Err(format!(
                "face set {} has 2 voltage or current loads: give each terminal 1",
                model.faces[t.faces].name
            ));
        }
    }
    if let Some(h) = &model.heat {
        if model.kind != Kind::Conduction {
            return Err("only a conduction model has a Joule heat".into());
        }
        if h.parts != model.parts || h.spacing != model.spacing || h.axis != model.axis {
            return Err("the heat model must have the parts and the grid of the model".into());
        }
        if h.step != heat::Step::Steady || h.symmetry != [false; 3] {
            return Err("the Joule heat goes into a steady heat solve with no symmetry".into());
        }
    }
    if !(model.scale > 0.0 && model.reference.is_finite()) {
        return Err("the unit of length must be above 0 and the reference finite".into());
    }
    Ok(())
}

/// The temperature of the steady heat solve.
struct HeatRun<'a> {
    outer: &'a mut dyn Events,
    temperature: Option<Vec<f64>>,
}

impl heat::Events for HeatRun<'_> {
    fn cancelled(&self) -> bool {
        self.outer.cancelled()
    }
    fn mesh(&mut self, _: &Mesh) -> Result<(), String> {
        Ok(())
    }
    fn progress(&mut self, text: &str) {
        self.outer.progress(&format!("heat: {text}"));
    }
    fn frame(&mut self, frame: &heat::Frame) -> Result<(), String> {
        self.temperature = Some(frame.temperature.clone());
        Ok(())
    }
}
