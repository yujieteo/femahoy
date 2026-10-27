//! Nonlinear heat conduction: a radiation to the ambient on named faces, and a conductivity and
//! a specific heat that change with the temperature (a table with linear interpolation).
//!
//! The radiation of a face into surroundings at the temperature T_amb that are much larger than
//! the face is q = eps sigma (T^4 - T_amb^4), with the absolute temperature T. Each point of the
//! face gets its share of the face area (a lumped face mass), so the term is diagonal: the
//! residual has eps sigma A_i (T_i^4 - T_amb^4) and the tangent 4 eps sigma A_i T_i^3. Below 0
//! the solver uses T |T|^3, which keeps the term monotone while Newton moves through the
//! unphysical range.
//!
//! The conductivity of a cell is the table value at the mean temperature of its points, and
//! multiplies the element matrix of a unit conductivity. In a slab of linear elements this is
//! exact at the points: the flux of a cell, k(T_m) (T_a - T_b) / h, equals the Kirchhoff value
//! (Phi(T_a) - Phi(T_b)) / h with Phi = integral of k, when k is linear in T. The stored heat is
//! the lumped enthalpy H(T) = integral of rho c dT, and a backward Euler step has the rate
//! (H(T) - H(T_previous)) / dt, so the energy balance is exact for each step.
//!
//! The iteration is Newton on the residual R(T) = K(T) T - f - q_rad(T) + rate(T), with a
//! backtracking line search on |R|. The tangent has the radiation and the capacity terms, and
//! K(T) without the term of dk/dT: that term makes the tangent unsymmetric, and the solvers of
//! the crate are for symmetric positive definite matrices. Thus the iteration converges
//! quadratically for the radiation and the capacity, and linearly (the Picard rate) for k(T).
//! Each iteration makes a new solver, so the multigrid preconditioner is of the current tangent.
//! The steady iteration starts at the mean ambient of the radiation faces: then the first
//! iteration is the linear solve with the film coefficient 4 eps sigma T_amb^3.
//!
//! Sources: F. P. Incropera, D. P. DeWitt, T. L. Bergman and A. S. Lavine, Fundamentals of Heat
//! and Mass Transfer, 6th ed., Wiley 2007, chapter 1 (radiation exchange with large
//! surroundings, the linearized radiation coefficient) and section 3.1 (the plane wall, where
//! the Kirchhoff transform Phi of a conductivity that changes with the temperature is linear);
//! J. R. Howell, A Catalog of Radiation Heat
//! Transfer Configuration Factors, http://www.thermalradiation.net/indexCat.html (the law of a
//! gray surface); K. J. Bathe, MIT 2.094 Finite Element Analysis of Solids and Fluids, lecture
//! notes "Field problems" (the Newton iteration of nonlinear heat transfer); J. E. Dennis and
//! R. B. Schnabel, Numerical Methods for Unconstrained Optimization and Nonlinear Equations,
//! SIAM 1996, section 6.3 (the backtracking line search).

use crate::heat::Mesh;
use crate::solve::{Method, Solver};
use crate::sparse::Csr;

/// A property against the temperature, with linear interpolation between the rows and the end
/// value beyond the ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// The temperatures, in increasing order.
    pub temperature: Vec<f64>,
    pub value: Vec<f64>,
}

impl Table {
    /// The error of a table that is not 2 or more rows of finite numbers in increasing order of
    /// the temperature, with values above 0.
    pub fn check(&self) -> Result<(), String> {
        let (t, v) = (&self.temperature, &self.value);
        if t.len() < 2 || t.len() != v.len() {
            return Err("a table needs 2 or more temperatures and 1 value for each".into());
        }
        if t.iter().chain(v).any(|x| !x.is_finite()) {
            return Err("a table needs finite numbers".into());
        }
        if t.windows(2).any(|w| !(w[0] < w[1])) {
            return Err("the temperatures of a table must increase".into());
        }
        if v.iter().any(|x| !(*x > 0.0)) {
            return Err("the values of a table must be above 0".into());
        }
        Ok(())
    }

    /// The value at the temperature `t`.
    pub fn at(&self, t: f64) -> f64 {
        let (ts, vs) = (&self.temperature, &self.value);
        let last = ts.len() - 1;
        if !(t > ts[0]) {
            return vs[0];
        }
        if t >= ts[last] {
            return vs[last];
        }
        let i = ts.partition_point(|&x| x <= t) - 1;
        let f = (t - ts[i]) / (ts[i + 1] - ts[i]);
        vs[i] + f * (vs[i + 1] - vs[i])
    }

    /// The integral of the value from the first temperature to `t`.
    pub fn integral(&self, t: f64) -> f64 {
        let (ts, vs) = (&self.temperature, &self.value);
        if !(t > ts[0]) {
            return vs[0] * (t - ts[0]);
        }
        let mut sum = 0.0;
        for i in 0..ts.len() - 1 {
            if t <= ts[i + 1] {
                return sum + 0.5 * (t - ts[i]) * (vs[i] + self.at(t));
            }
            sum += 0.5 * (ts[i + 1] - ts[i]) * (vs[i] + vs[i + 1]);
        }
        sum + vs[ts.len() - 1] * (t - ts[ts.len() - 1])
    }
}

/// Newton stops when the largest change of a temperature is below this fraction of the largest
/// temperature. Reason: the slab with radiation (test `radiation_slab`) needs its face
/// temperature to 1e-8 of the root of the scalar equation, and the PCG solve of each iteration
/// has the relative residual 1e-10.
const TOL: f64 = 1e-10;

/// The most Newton iterations of 1 solve. Reason: the tests converge in 4 to 25 iterations; a
/// solve that needs more does not converge.
const MAX_ITERATIONS: usize = 100;

/// The terms of a nonlinear model on its mesh.
pub(crate) struct Nonlinear<'a> {
    pub mesh: &'a Mesh,
    /// The conductivity matrix with the convection, at a unit conductivity in each cell of a
    /// table material.
    pub k: &'a Csr,
    /// The cells of a table material and their table.
    pub cells: Vec<(usize, &'a Table)>,
    /// The element matrix of a cell at a unit conductivity.
    pub element: &'a dyn Fn(usize) -> Vec<f64>,
    /// The heat into each point from the sources, the fluxes and the convection sinks.
    pub load: &'a [f64],
    /// The lumped capacity of each point from the materials with a constant capacity.
    pub capacity: &'a [f64],
    /// The lumped volume of a point in a cell of a material with a capacity table.
    pub heat: Vec<(u32, &'a Table, f64)>,
    /// The radiation of a point: eps sigma times its share of the face area, and the ambient.
    pub radiation: Vec<(u32, f64, f64)>,
    pub free: &'a [usize],
}

/// The heat flows of a temperature field.
pub(crate) struct State {
    /// K(T), with the convection.
    pub matrix: Csr,
    /// K(T) T.
    pub kt: Vec<f64>,
    /// The rate of the stored heat at each point.
    pub rate: Vec<f64>,
    /// The radiation into the body at each point.
    pub radiation: Vec<f64>,
    /// K(T) T - load - radiation + rate.
    pub residual: Vec<f64>,
    /// The conductivity of each cell of `Nonlinear::cells`.
    pub scale: Vec<f64>,
}

/// The work of 1 nonlinear solve.
pub(crate) struct Report {
    pub iterations: usize,
    pub method: Method,
    /// The largest change of a temperature in the last iteration.
    pub change: f64,
}

impl Nonlinear<'_> {
    /// The heat flows at the temperatures `t`, with the previous temperatures and the time step
    /// of a transient step.
    pub fn state(&self, t: &[f64], previous: Option<(&[f64], f64)>) -> State {
        let n = t.len();
        let mut matrix = self.k.clone();
        let mut scale = Vec::with_capacity(self.cells.len());
        for &(e, table) in &self.cells {
            let nodes = self.mesh.cell(e);
            let mean = nodes.iter().map(|&v| t[v as usize]).sum::<f64>() / nodes.len() as f64;
            let k = table.at(mean);
            let ke: Vec<f64> = (self.element)(e).iter().map(|v| (k - 1.0) * v).collect();
            matrix.add_element(nodes, &ke);
            scale.push(k);
        }
        let mut kt = vec![0.0; n];
        matrix.mul(t, &mut kt);
        let mut rate = vec![0.0; n];
        if let Some((p, dt)) = previous {
            for i in 0..n {
                rate[i] = self.capacity[i] * (t[i] - p[i]) / dt;
            }
            for &(v, table, w) in &self.heat {
                let v = v as usize;
                rate[v] += w * (table.integral(t[v]) - table.integral(p[v])) / dt;
            }
        }
        let mut radiation = vec![0.0; n];
        for &(v, c, ambient) in &self.radiation {
            let x = t[v as usize];
            radiation[v as usize] += c * (ambient.powi(4) - x * x.abs().powi(3));
        }
        let residual = (0..n)
            .map(|i| kt[i] - self.load[i] - radiation[i] + rate[i])
            .collect();
        State {
            matrix,
            kt,
            rate,
            radiation,
            residual,
            scale,
        }
    }

    /// The tangent of the residual at `t` on the free points.
    fn tangent(&self, state: &State, t: &[f64], dt: Option<f64>) -> Csr {
        let mut j = state.matrix.clone();
        let mut add = |i: usize, v: f64| {
            let d = j.find(i, i).expect("a diagonal");
            j.val[d] += v;
        };
        for &(v, c, _) in &self.radiation {
            add(v as usize, 4.0 * c * t[v as usize].abs().powi(3));
        }
        if let Some(dt) = dt {
            for (i, c) in self.capacity.iter().enumerate() {
                add(i, c / dt);
            }
            for &(v, table, w) in &self.heat {
                add(v as usize, w * table.at(t[v as usize]) / dt);
            }
        }
        j.submatrix(self.free)
    }

    fn norm(&self, state: &State) -> f64 {
        self.free
            .iter()
            .map(|&i| state.residual[i] * state.residual[i])
            .sum::<f64>()
            .sqrt()
    }

    /// Solve R(T) = 0 for the free points of `t`, from `t`. `solver` makes the linear solver of
    /// a tangent.
    pub fn solve(
        &self,
        t: &mut [f64],
        previous: Option<(&[f64], f64)>,
        solver: &dyn Fn(Csr) -> Result<Solver, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(State, Report), String> {
        let mut state = self.state(t, previous);
        let mut trial = t.to_vec();
        let mut change = f64::INFINITY;
        for iteration in 1..=MAX_ITERATIONS {
            let s = solver(self.tangent(&state, t, previous.map(|p| p.1)))?;
            let rhs: Vec<f64> = self.free.iter().map(|&i| -state.residual[i]).collect();
            let mut dx = vec![0.0; rhs.len()];
            s.solve(&rhs, &mut dx, cancelled)?;
            let r0 = self.norm(&state);
            let mut alpha = 1.0;
            let next = loop {
                for (k, &i) in self.free.iter().enumerate() {
                    trial[i] = t[i] + alpha * dx[k];
                }
                let next = self.state(&trial, previous);
                if self.norm(&next) <= (1.0 - 1e-4 * alpha) * r0 || alpha < 1.0 / 256.0 {
                    break next;
                }
                alpha *= 0.5;
            };
            t.copy_from_slice(&trial);
            state = next;
            change = alpha * dx.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            let size = t.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            if change <= TOL * size || r0 == 0.0 {
                let report = Report {
                    iterations: iteration,
                    method: s.method(),
                    change,
                };
                return Ok((state, report));
            }
        }
        Err(format!(
            "the nonlinear solve did not converge in {MAX_ITERATIONS} Newton iterations (the last \
             change of a temperature is {change:.2e}). With a radiation, give the temperatures in \
             kelvin; for a transient step, use a smaller dt"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_interpolates_and_integrates() {
        let t = Table {
            temperature: vec![300.0, 400.0, 600.0],
            value: vec![10.0, 20.0, 10.0],
        };
        assert_eq!(t.at(200.0), 10.0);
        assert_eq!(t.at(350.0), 15.0);
        assert_eq!(t.at(500.0), 15.0);
        assert_eq!(t.at(700.0), 10.0);
        assert_eq!(t.integral(250.0), -500.0);
        assert_eq!(t.integral(350.0), 625.0);
        assert_eq!(t.integral(600.0), 1500.0 + 3000.0);
        assert_eq!(t.integral(650.0), 4500.0 + 500.0);
        assert!(t.check().is_ok());
        let bad = Table {
            temperature: vec![300.0, 300.0],
            value: vec![1.0, 1.0],
        };
        assert!(bad.check().is_err());
    }
}
