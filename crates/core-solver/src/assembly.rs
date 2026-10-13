//! Assembly of the reduced matrix `Tᵀ K T` from element matrices through a slot map.
//!
//! The map is built once for a matrix pattern. For each element it holds the position in
//! the matrix values of every entry of the element matrix, so an element adds into the
//! matrix with no search. An element that touches a constrained dof is kept as a list of
//! (row, column, slot, coefficient) terms. The [`Assemble`] trait is the one place that
//! adds element matrices to the values. A backend with another device (for example Metal)
//! replaces it. The trait works on flat arrays and the map only, and it names no device.

use std::collections::HashMap;

use crate::constraint::Transform;
use crate::element::{Evaluation, MAX_DOFS};
use crate::sparse::Matrix;

/// Marks an entry of a plain element that falls on a fixed dof. It adds nothing.
const SKIP: u32 = u32::MAX;

/// A term of an element that touches a constrained dof.
#[derive(Debug, Clone, Copy)]
struct Term {
    i: u8,
    j: u8,
    slot: u32,
    coef: f64,
}

/// Where each entry of each element matrix goes.
pub struct SlotMap {
    /// First entry of each element in `slots`, and one past the end.
    start: Vec<usize>,
    /// For a plain element, the slots of the lower triangle of its matrix, row by row:
    /// entry `(i, j)` with `i >= j` is at `i * (i + 1) / 2 + j`.
    slots: Vec<u32>,
    /// Elements that touch a constrained dof. Their `start` range is empty.
    terms: HashMap<usize, Vec<Term>>,
}

impl SlotMap {
    /// Bytes of the map.
    pub fn bytes(&self) -> usize {
        self.start.len() * 8 + self.slots.len() * 4
    }

    /// Build the map. `dofs_of(e, buf)` returns the number of dofs of element `e` and writes the
    /// full dofs into `buf`.
    pub fn build(
        matrix: &Matrix,
        t: &Transform,
        n_elements: usize,
        dofs_of: &dyn Fn(usize, &mut [usize; MAX_DOFS]) -> usize,
    ) -> SlotMap {
        let mut start = Vec::with_capacity(n_elements + 1);
        let mut slots = Vec::new();
        let mut terms = HashMap::new();
        let mut dofs = [0; MAX_DOFS];
        for e in 0..n_elements {
            start.push(slots.len());
            let n = dofs_of(e, &mut dofs);
            // A dof is plain when it maps to one reduced dof with the factor 1, or to none.
            let mut red = [None::<u32>; MAX_DOFS];
            let mut plain = true;
            for (r, &d) in red.iter_mut().zip(&dofs[..n]) {
                let (cols, coefs) = t.row(d);
                match (cols, coefs) {
                    ([], _) => *r = None,
                    ([c], [k]) if *k == 1.0 => *r = Some(*c),
                    _ => plain = false,
                }
            }
            // Two dofs of one element on one reduced dof need the full loop.
            if plain {
                'dup: for i in 0..n {
                    for j in 0..i {
                        if red[i].is_some() && red[i] == red[j] {
                            plain = false;
                            break 'dup;
                        }
                    }
                }
            }
            if plain {
                for i in 0..n {
                    for j in 0..=i {
                        slots.push(match (red[i], red[j]) {
                            (Some(a), Some(b)) => {
                                let (a, b) = if a >= b { (a, b) } else { (b, a) };
                                matrix.position(a, b).unwrap_or(SKIP)
                            }
                            _ => SKIP,
                        });
                    }
                }
            } else {
                let mut list = Vec::new();
                for i in 0..n {
                    let (ca, ka) = t.row(dofs[i]);
                    for j in 0..n {
                        let (cb, kb) = t.row(dofs[j]);
                        for (&a, &xa) in ca.iter().zip(ka) {
                            for (&b, &xb) in cb.iter().zip(kb) {
                                if a >= b {
                                    if let Some(slot) = matrix.position(a, b) {
                                        list.push(Term {
                                            i: i as u8,
                                            j: j as u8,
                                            slot,
                                            coef: xa * xb,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                terms.insert(e, list);
            }
        }
        start.push(slots.len());
        SlotMap { start, slots, terms }
    }
}

/// Adds element matrices to the values of the matrix. The sum for one entry runs in the
/// order of the elements, so the result does not depend on the number of threads.
pub trait Assemble: Sync {
    /// Add the matrices of the elements `first..first + evals.len()`. `evals[k].k` holds the
    /// matrix of element `first + k`, and `evals[k].n` its size.
    fn add_chunk(&self, values: &mut [f64], map: &SlotMap, first: usize, evals: &[Evaluation]);
}

/// The reference backend: one thread adds the chunk in element order.
pub struct CpuAssembler;

impl Assemble for CpuAssembler {
    fn add_chunk(&self, values: &mut [f64], map: &SlotMap, first: usize, evals: &[Evaluation]) {
        for (k, ev) in evals.iter().enumerate() {
            let e = first + k;
            if let Some(list) = map.terms.get(&e) {
                for t in list {
                    values[t.slot as usize] += t.coef * ev.k[usize::from(t.i)][usize::from(t.j)];
                }
                continue;
            }
            let slots = &map.slots[map.start[e]..map.start[e + 1]];
            let mut at = 0;
            for i in 0..ev.n {
                for j in 0..=i {
                    let s = slots[at];
                    at += 1;
                    if s != SKIP {
                        values[s as usize] += ev.k[i][j];
                    }
                }
            }
        }
    }
}
