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
//! backtracking line search on |R|. The tangent has the radiation and the capacity terms, K(T),
//! and the term of dk/dT: a cell e with n_e points and the mean temperature T_m adds
//! k'(T_m) / n_e (K_e T_e)_a to each column of each row a of the cell, with the element matrix
//! K_e of a unit conductivity. That term makes the tangent unsymmetric, so flexible GMRES solves
//! it, with the solver of the symmetric part (K(T) and the diagonal terms) as the preconditioner.
//! With no table of the conductivity, the preconditioner is the tangent, and GMRES takes 1
//! iteration. Thus the iteration converges quadratically. Each iteration makes a new solver, so
//! the multigrid preconditioner is of the current tangent. The steady iteration starts at the
//! mean ambient of the radiation faces: then the first iteration is the linear solve with the
//! film coefficient 4 eps sigma T_amb^3.
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
//! SIAM 1996, section 6.3 (the backtracking line search); Y. Saad, Iterative Methods for Sparse
//! Linear Systems, 2nd ed., SIAM 2003, sections 6.5 (restarted GMRES with Givens rotations) and
//! 9.4.1 (flexible GMRES, with a preconditioner that is an inner iteration).

use crate::heat::Mesh;
use crate::solve::{Method, Solver, PCG_TOL};
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

    /// The derivative of the value at the temperature `t`: the slope of the row interval that
    /// holds `t`, and 0 beyond the ends.
    pub fn slope(&self, t: f64) -> f64 {
        let (ts, vs) = (&self.temperature, &self.value);
        if !(t >= ts[0]) || t >= ts[ts.len() - 1] {
            return 0.0;
        }
        let i = ts.partition_point(|&x| x <= t) - 1;
        (vs[i + 1] - vs[i]) / (ts[i + 1] - ts[i])
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

/// Newton stops when the norm of the residual on the free points is below this fraction of the
/// gross heat flow (`Nonlinear::gross`), which does not grow with the number of points. Reason: the slab with radiation (test
/// `a_slab_that_radiates_has_the_root_of_its_energy_balance`) needs its face temperature to 1e-8
/// of the root of the scalar equation, and the linear solve of each iteration has the relative
/// residual 1e-10.
const TOL: f64 = 1e-10;

/// The most Newton iterations of 1 solve. Reason: the tests and the validation cases converge in
/// 2 to 6 iterations; a solve that needs more does not converge.
const MAX_ITERATIONS: usize = 100;

/// The Krylov vectors of GMRES before a restart, and the most GMRES iterations of 1 Newton
/// iteration. Reason: the tests take 1 to 8 GMRES iterations for each Newton iteration, so 30
/// vectors hold them with no restart.
const RESTART: usize = 30;
const GMRES_MAX_ITERATIONS: usize = 300;

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
    /// The norm of the residual on the free points over the gross heat flow.
    pub residual: f64,
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

    /// The symmetric part of the tangent of the residual at `t` on the free points, and the
    /// tangent.
    fn tangent(&self, state: &State, t: &[f64], dt: Option<f64>) -> (Csr, Csr) {
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
        let symmetric = j.submatrix(self.free);
        for &(e, table) in &self.cells {
            let nodes = self.mesh.cell(e);
            let m = nodes.len();
            let mean = nodes.iter().map(|&v| t[v as usize]).sum::<f64>() / m as f64;
            let slope = table.slope(mean) / m as f64;
            if slope == 0.0 {
                continue;
            }
            let ke = (self.element)(e);
            let mut de = vec![0.0; m * m];
            for a in 0..m {
                let flow: f64 = (0..m).map(|b| ke[a * m + b] * t[nodes[b] as usize]).sum();
                de[a * m..(a + 1) * m].fill(slope * flow);
            }
            j.add_element(nodes, &de);
        }
        (symmetric, j.submatrix(self.free))
    }

    fn norm(&self, state: &State) -> f64 {
        self.free
            .iter()
            .map(|&i| state.residual[i] * state.residual[i])
            .sum::<f64>()
            .sqrt()
    }

    /// The gross heat flow: the sum of the absolute heat flows into the body from the loads,
    /// the radiation and the fixed temperatures, and of the rate of the stored heat.
    fn gross(&self, state: &State) -> f64 {
        let mut fixed = vec![true; state.residual.len()];
        for &i in self.free {
            fixed[i] = false;
        }
        (0..fixed.len())
            .map(|i| {
                let reaction = if fixed[i] {
                    state.residual[i].abs()
                } else {
                    0.0
                };
                self.load[i].abs() + state.radiation[i].abs() + state.rate[i].abs() + reaction
            })
            .sum()
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
        let mut residual = f64::INFINITY;
        for iteration in 1..=MAX_ITERATIONS {
            let (symmetric, tangent) = self.tangent(&state, t, previous.map(|p| p.1));
            let s = solver(symmetric)?;
            let rhs: Vec<f64> = self.free.iter().map(|&i| -state.residual[i]).collect();
            let mut dx = vec![0.0; rhs.len()];
            gmres(
                &tangent,
                &rhs,
                &mut dx,
                &|v, z| {
                    z.fill(0.0);
                    s.solve(v, z, cancelled).map(|_| ())
                },
                cancelled,
            )?;
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
            let (norm, gross) = (self.norm(&state), self.gross(&state));
            residual = if gross > 0.0 { norm / gross } else { norm };
            if norm <= TOL * gross {
                let report = Report {
                    iterations: iteration,
                    method: s.method(),
                    residual,
                };
                return Ok((state, report));
            }
        }
        Err(format!(
            "the nonlinear solve did not converge in {MAX_ITERATIONS} Newton iterations (the last \
             residual is {residual:.2e} of the gross heat flow). With a radiation, give the \
             temperatures in kelvin; for a transient step, use a smaller dt"
        ))
    }
}

/// z = M^-1 v of a preconditioner M.
type Preconditioner<'a> = dyn Fn(&[f64], &mut [f64]) -> Result<(), String> + 'a;

/// Solve A x = b by restarted flexible GMRES with the preconditioner `m` (z = M^-1 v), from the
/// start `x`, to the relative residual of the PCG solves. The answer is the number of iterations.
/// `cancelled` is checked at each restart.
fn gmres(
    a: &Csr,
    b: &[f64],
    x: &mut [f64],
    m: &Preconditioner,
    cancelled: &dyn Fn() -> bool,
) -> Result<usize, String> {
    let n = b.len();
    let norm = |v: &[f64]| v.iter().map(|e| e * e).sum::<f64>().sqrt();
    let bnorm = norm(b);
    if bnorm == 0.0 {
        x.fill(0.0);
        return Ok(0);
    }
    let mut done = 0;
    let mut w = vec![0.0; n];
    loop {
        if cancelled() {
            return Err("cancelled".into());
        }
        a.mul(x, &mut w);
        let r: Vec<f64> = b.iter().zip(&w).map(|(p, q)| p - q).collect();
        let beta = norm(&r);
        if beta <= PCG_TOL * bnorm {
            return Ok(done);
        }
        if done >= GMRES_MAX_ITERATIONS {
            return Err(format!(
                "GMRES did not converge in {GMRES_MAX_ITERATIONS} iterations: the residual is \
                 {:.1e} of the right side",
                beta / bnorm
            ));
        }
        // The Krylov vectors, their preconditioned vectors, the Hessenberg matrix by columns
        // after the rotations, the rotations, and the right side of the least squares problem.
        let mut v: Vec<Vec<f64>> = vec![r.iter().map(|e| e / beta).collect()];
        let mut z: Vec<Vec<f64>> = Vec::new();
        let mut h: Vec<Vec<f64>> = Vec::new();
        let mut rot: Vec<(f64, f64)> = Vec::new();
        let mut g = vec![beta];
        for j in 0..RESTART {
            let mut zj = vec![0.0; n];
            m(&v[j], &mut zj)?;
            a.mul(&zj, &mut w);
            z.push(zj);
            let mut col = Vec::with_capacity(j + 2);
            for vi in &v {
                let hij: f64 = vi.iter().zip(&w).map(|(p, q)| p * q).sum();
                for (p, q) in w.iter_mut().zip(vi) {
                    *p -= hij * q;
                }
                col.push(hij);
            }
            let hn = norm(&w);
            col.push(hn);
            for (i, &(c, s)) in rot.iter().enumerate() {
                let (p, q) = (col[i], col[i + 1]);
                col[i] = c * p + s * q;
                col[i + 1] = c * q - s * p;
            }
            let (p, q) = (col[j], col[j + 1]);
            let d = p.hypot(q);
            let (c, s) = (p / d, q / d);
            col[j] = d;
            col[j + 1] = 0.0;
            rot.push((c, s));
            g.push(-s * g[j]);
            g[j] *= c;
            h.push(col);
            done += 1;
            let converged = g[j + 1].abs() <= PCG_TOL * bnorm;
            if converged || done >= GMRES_MAX_ITERATIONS || hn == 0.0 {
                break;
            }
            v.push(w.iter().map(|e| e / hn).collect());
        }
        // y = H^-1 g, then x += Z y.
        let k = h.len();
        let mut y = vec![0.0; k];
        for i in (0..k).rev() {
            let s: f64 = (i + 1..k).map(|c| h[c][i] * y[c]).sum();
            y[i] = (g[i] - s) / h[i][i];
        }
        for (zk, yk) in z.iter().zip(&y) {
            for (p, q) in x.iter_mut().zip(zk) {
                *p += q * yk;
            }
        }
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
        assert_eq!(t.slope(250.0), 0.0);
        assert_eq!(t.slope(300.0), 0.1);
        assert_eq!(t.slope(500.0), -0.05);
        assert_eq!(t.slope(600.0), 0.0);
        assert!(t.check().is_ok());
        let bad = Table {
            temperature: vec![300.0, 300.0],
            value: vec![1.0, 1.0],
        };
        assert!(bad.check().is_err());
    }

    #[test]
    fn gmres_solves_a_nonsymmetric_matrix() {
        let mut a = Csr::pattern(3, std::iter::once(&[0u32, 1, 2][..]));
        a.add_element(&[0, 1, 2], &[4.0, 1.0, 0.0, -2.0, 3.0, 1.0, 1.0, -1.0, 5.0]);
        let exact = [1.0, -2.0, 3.0];
        let mut b = vec![0.0; 3];
        a.mul(&exact, &mut b);
        let mut x = vec![0.0; 3];
        let identity = |v: &[f64], z: &mut [f64]| {
            z.copy_from_slice(v);
            Ok(())
        };
        let iterations = gmres(&a, &b, &mut x, &identity, &|| false).unwrap();
        assert!(iterations <= 3, "{iterations}");
        for (p, q) in x.iter().zip(exact) {
            assert!((p - q).abs() < 1e-9, "{p} against {q}");
        }
    }
}
