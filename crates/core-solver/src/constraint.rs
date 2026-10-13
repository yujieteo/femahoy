//! DOF elimination with a master-slave transformation.
//!
//! Each linear constraint `sum(c_j * u_j) = g` removes one slave dof. The full
//! vector is `u = T * u_r + u_0`, with `u_r` the free dofs. The reduced matrix
//! `Tᵀ K T` stays symmetric positive definite when `K` is.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::SolverError;

#[derive(Debug, Clone)]
struct Slave {
    /// `u_slave = sum(coef * u_master) + offset`. Masters are never slaves.
    terms: BTreeMap<usize, f64>,
    offset: f64,
}

pub struct Elimination {
    n: usize,
    slaves: HashMap<usize, Slave>,
    /// For each master dof, the slaves whose definition uses it.
    users: HashMap<usize, HashSet<usize>>,
}

/// The result of the elimination. The rows are stored in compressed form.
#[derive(Debug, Clone)]
pub struct Transform {
    pub n_free: usize,
    row_ptr: Vec<u32>,
    cols: Vec<u32>,
    coefs: Vec<f64>,
    /// The part of each full dof that does not depend on `u_r`.
    pub offset: Vec<f64>,
    /// The full dof of each reduced dof.
    free_dof: Vec<u32>,
}

const DROP: f64 = 1e-13;

/// A NaN or an infinite value would change or remove a constraint without an error.
fn not_finite() -> SolverError {
    SolverError::new("a constraint has a coefficient or a value that is not finite")
}

impl Elimination {
    pub fn new(n: usize) -> Self {
        Elimination {
            n,
            slaves: HashMap::new(),
            users: HashMap::new(),
        }
    }

    /// Add `sum(c * u) = rhs`. `prefer` names the dof to eliminate when it can be.
    /// A redundant equation is dropped. An equation that contradicts the earlier
    /// ones is an error.
    pub fn add(
        &mut self,
        row: &[(usize, f64)],
        rhs: f64,
        prefer: Option<usize>,
    ) -> Result<(), SolverError> {
        if !rhs.is_finite() || row.iter().any(|(_, c)| !c.is_finite()) {
            return Err(not_finite());
        }
        let mut acc: BTreeMap<usize, f64> = BTreeMap::new();
        let mut rhs_now = rhs;
        let mut scale = 1.0 + rhs.abs();
        for &(j, c) in row {
            debug_assert!(j < self.n);
            match self.slaves.get(&j) {
                Some(s) => {
                    for (&m, &t) in &s.terms {
                        *acc.entry(m).or_insert(0.0) += c * t;
                    }
                    rhs_now -= c * s.offset;
                    scale += (c * s.offset).abs();
                }
                None => *acc.entry(j).or_insert(0.0) += c,
            }
        }
        // The substitution of earlier slaves can overflow.
        if !rhs_now.is_finite() || acc.values().any(|c| !c.is_finite()) {
            return Err(not_finite());
        }
        let max = acc.values().fold(0.0_f64, |m, c| m.max(c.abs()));
        acc.retain(|_, c| c.abs() > DROP * max);
        if acc.is_empty() {
            if rhs_now.abs() > 1e-9 * scale {
                return Err(SolverError::new(format!(
                    "conflicting constraints: residual {rhs_now:e}"
                )));
            }
            return Ok(());
        }
        let pivot = match prefer.filter(|p| acc.get(p).is_some_and(|c| c.abs() >= 1e-8 * max)) {
            Some(p) => p,
            None => *acc
                .iter()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()).then(b.0.cmp(a.0)))
                .map(|(k, _)| k)
                .unwrap_or(&0),
        };
        let a = acc.remove(&pivot).unwrap_or(1.0);
        let def = Slave {
            terms: acc.into_iter().map(|(k, c)| (k, -c / a)).collect(),
            offset: rhs_now / a,
        };
        if !def.offset.is_finite() || def.terms.values().any(|c| !c.is_finite()) {
            return Err(not_finite());
        }
        // Replace the new slave in the definitions that use it.
        if let Some(users) = self.users.remove(&pivot) {
            for s in users {
                let Some(slave) = self.slaves.get_mut(&s) else {
                    continue;
                };
                let t = slave.terms.remove(&pivot).unwrap_or(0.0);
                slave.offset += t * def.offset;
                for (&m, &c) in &def.terms {
                    *slave.terms.entry(m).or_insert(0.0) += t * c;
                    self.users.entry(m).or_default().insert(s);
                }
                if !slave.offset.is_finite() || slave.terms.values().any(|c| !c.is_finite()) {
                    return Err(not_finite());
                }
            }
        }
        for &m in def.terms.keys() {
            self.users.entry(m).or_default().insert(pivot);
        }
        self.slaves.insert(pivot, def);
        Ok(())
    }

    /// Build the transform from the constraints added so far. The elimination
    /// stays open: more constraints can follow, and `finish` can run again.
    pub fn finish(&self) -> Transform {
        let mut reduced = vec![u32::MAX; self.n];
        let mut free_dof = Vec::new();
        for (d, r) in reduced.iter_mut().enumerate() {
            if !self.slaves.contains_key(&d) {
                *r = free_dof.len() as u32;
                free_dof.push(d as u32);
            }
        }
        let n_free = free_dof.len();
        let mut row_ptr = Vec::with_capacity(self.n + 1);
        let mut cols = Vec::with_capacity(self.n);
        let mut coefs = Vec::with_capacity(self.n);
        let mut offset = vec![0.0; self.n];
        row_ptr.push(0);
        for d in 0..self.n {
            match self.slaves.get(&d) {
                None => {
                    cols.push(reduced[d]);
                    coefs.push(1.0);
                }
                Some(s) => {
                    offset[d] = s.offset;
                    for (&m, &c) in s.terms.iter().filter(|(_, c)| **c != 0.0) {
                        cols.push(reduced[m]);
                        coefs.push(c);
                    }
                }
            }
            row_ptr.push(cols.len() as u32);
        }
        Transform {
            n_free,
            row_ptr,
            cols,
            coefs,
            offset,
            free_dof,
        }
    }
}

impl Transform {
    /// Number of full dofs.
    pub fn n_full(&self) -> usize {
        self.offset.len()
    }

    /// The reduced dofs and coefficients that full dof `d` depends on.
    pub fn row(&self, d: usize) -> (&[u32], &[f64]) {
        let (a, b) = (self.row_ptr[d] as usize, self.row_ptr[d + 1] as usize);
        (&self.cols[a..b], &self.coefs[a..b])
    }

    /// The full dof that reduced dof `r` stands for.
    pub fn full_dof(&self, r: usize) -> usize {
        self.free_dof[r] as usize
    }

    /// True if both transforms have the same reduced dofs and the same
    /// dependencies. The coefficients and the offsets may differ. Then a matrix
    /// pattern built for one transform fits the other.
    pub fn same_structure(&self, other: &Transform) -> bool {
        self.n_free == other.n_free && self.row_ptr == other.row_ptr && self.cols == other.cols
    }

    /// True if both transforms map the reduced dofs the same way. Only the offsets may
    /// differ. Then a matrix and its factor built for one transform fit the other.
    pub fn same_map(&self, other: &Transform) -> bool {
        self.same_structure(other) && self.coefs == other.coefs
    }

    /// `T * u_r + u_0`.
    pub fn expand(&self, reduced: &[f64]) -> Vec<f64> {
        (0..self.n_full())
            .map(|d| {
                let (c, k) = self.row(d);
                self.offset[d]
                    + c.iter()
                        .zip(k)
                        .map(|(&r, k)| k * reduced[r as usize])
                        .sum::<f64>()
            })
            .collect()
    }

    /// `Tᵀ * f`.
    pub fn reduce(&self, full: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.n_free];
        for (d, f) in full.iter().enumerate() {
            let (c, k) = self.row(d);
            for (&r, k) in c.iter().zip(k) {
                out[r as usize] += k * f;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prescribed_value_becomes_offset() {
        let mut e = Elimination::new(3);
        e.add(&[(1, 1.0)], 0.5, Some(1)).unwrap();
        let t = e.finish();
        assert_eq!(t.n_free, 2);
        assert_eq!(t.expand(&[10.0, 20.0]), vec![10.0, 0.5, 20.0]);
    }

    #[test]
    fn equation_chain_resolves_to_masters() {
        // u1 = 2 u0, u2 = u1 + 1  =>  u2 = 2 u0 + 1.
        let mut e = Elimination::new(3);
        e.add(&[(1, 1.0), (0, -2.0)], 0.0, Some(1)).unwrap();
        e.add(&[(2, 1.0), (1, -1.0)], 1.0, Some(2)).unwrap();
        let t = e.finish();
        assert_eq!(t.n_free, 1);
        assert_eq!(t.expand(&[3.0]), vec![3.0, 6.0, 7.0]);
    }

    #[test]
    fn later_prescribed_value_flows_to_the_slave() {
        // u1 = 2 u0 first, then u0 = 4: u1 is 8.
        let mut e = Elimination::new(2);
        e.add(&[(1, 1.0), (0, -2.0)], 0.0, Some(1)).unwrap();
        e.add(&[(0, 1.0)], 4.0, Some(0)).unwrap();
        let t = e.finish();
        assert_eq!(t.n_free, 0);
        assert_eq!(t.expand(&[]), vec![4.0, 8.0]);
    }

    #[test]
    fn redundant_equation_is_dropped_and_conflict_is_an_error() {
        let mut e = Elimination::new(2);
        e.add(&[(0, 1.0)], 1.0, Some(0)).unwrap();
        e.add(&[(0, 2.0)], 2.0, Some(0)).unwrap();
        let err = e.add(&[(0, 1.0)], 3.0, Some(0)).unwrap_err();
        assert!(err.to_string().contains("conflicting constraints"));
    }

    #[test]
    fn a_value_that_is_not_finite_is_an_error() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut e = Elimination::new(2);
            assert!(e.add(&[(0, bad), (1, -1.0)], 0.0, None).is_err());
            assert!(e.add(&[(0, 1.0)], bad, None).is_err());
        }
        // Finite input can overflow in the substitution: u1 = 1e8 u0, then 1e305 u1 = u2.
        let mut e = Elimination::new(3);
        e.add(&[(1, 1e-8), (0, -1.0)], 0.0, Some(1)).unwrap();
        let err = e.add(&[(1, 1e305), (2, -1.0)], 0.0, Some(2)).unwrap_err();
        assert!(err.to_string().contains("not finite"), "{err}");
    }

    #[test]
    fn reduce_is_the_transpose_of_expand() {
        let mut e = Elimination::new(3);
        e.add(&[(1, 1.0), (0, -2.0), (2, -1.0)], 0.0, Some(1))
            .unwrap();
        let t = e.finish();
        // f . (T x) == (T^T f) . x for any x, f.
        let (f, x) = ([1.0, 2.0, 3.0], [0.7, -0.3]);
        let lhs: f64 = f.iter().zip(t.expand(&x)).map(|(a, b)| a * b).sum();
        let rhs: f64 = t.reduce(&f).iter().zip(x).map(|(a, b)| a * b).sum();
        assert!((lhs - rhs).abs() < 1e-14);
    }

    #[test]
    fn elimination_stays_open_after_finish() {
        let mut e = Elimination::new(3);
        e.add(&[(1, 1.0), (0, -2.0)], 0.0, Some(1)).unwrap();
        let first = e.finish();
        assert_eq!(first.n_free, 2);
        // A constraint added later changes the result of the next finish.
        e.add(&[(0, 1.0)], 4.0, Some(0)).unwrap();
        let second = e.finish();
        assert_eq!(second.n_free, 1);
        assert_eq!(second.expand(&[7.0]), vec![4.0, 8.0, 7.0]);
        assert!(!first.same_structure(&second));
    }

    #[test]
    fn same_structure_ignores_the_values() {
        let make = |rhs: f64, k: f64| {
            let mut e = Elimination::new(3);
            e.add(&[(1, 1.0), (0, -k)], rhs, Some(1)).unwrap();
            e.finish()
        };
        assert!(make(0.0, 2.0).same_structure(&make(1.0, 3.0)));
    }

    #[test]
    fn full_dof_names_the_free_dof() {
        let mut e = Elimination::new(4);
        e.add(&[(1, 1.0)], 0.0, Some(1)).unwrap();
        let t = e.finish();
        assert_eq!(
            (0..t.n_free).map(|r| t.full_dof(r)).collect::<Vec<_>>(),
            vec![0, 2, 3]
        );
    }
}
