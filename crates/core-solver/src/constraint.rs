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

/// The result of the elimination.
pub struct Transform {
    pub n_free: usize,
    /// For each full dof, the reduced dofs and coefficients it depends on.
    pub rows: Vec<Vec<(usize, f64)>>,
    /// The part of each full dof that does not depend on `u_r`.
    pub offset: Vec<f64>,
}

const DROP: f64 = 1e-13;

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
            }
        }
        for &m in def.terms.keys() {
            self.users.entry(m).or_default().insert(pivot);
        }
        self.slaves.insert(pivot, def);
        Ok(())
    }

    pub fn finish(self) -> Transform {
        let mut reduced = vec![usize::MAX; self.n];
        let mut n_free = 0;
        for (d, r) in reduced.iter_mut().enumerate() {
            if !self.slaves.contains_key(&d) {
                *r = n_free;
                n_free += 1;
            }
        }
        let mut rows = Vec::with_capacity(self.n);
        let mut offset = vec![0.0; self.n];
        for d in 0..self.n {
            match self.slaves.get(&d) {
                None => rows.push(vec![(reduced[d], 1.0)]),
                Some(s) => {
                    offset[d] = s.offset;
                    rows.push(
                        s.terms
                            .iter()
                            .filter(|(_, c)| **c != 0.0)
                            .map(|(&m, &c)| (reduced[m], c))
                            .collect(),
                    );
                }
            }
        }
        Transform {
            n_free,
            rows,
            offset,
        }
    }
}

impl Transform {
    /// `T * u_r + u_0`.
    pub fn expand(&self, reduced: &[f64]) -> Vec<f64> {
        self.rows
            .iter()
            .zip(&self.offset)
            .map(|(row, o)| o + row.iter().map(|&(r, c)| c * reduced[r]).sum::<f64>())
            .collect()
    }

    /// `Tᵀ * f`.
    pub fn reduce(&self, full: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.n_free];
        for (row, f) in self.rows.iter().zip(full) {
            for &(r, c) in row {
                out[r] += c * f;
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
}
