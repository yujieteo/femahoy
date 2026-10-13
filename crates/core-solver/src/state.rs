//! Material and element state with a committed copy and a trial copy.
//!
//! The Newton loop reads the committed copy and writes the trial copy. A converged
//! increment commits the trial copy. A cutback drops it. The store is flat and
//! uses one slab of numbers for each element. Inside a slab the layout is
//! component-major: number `c` of point `i` of an element with `np` points is at
//! `c * np + i`. This is the struct-of-arrays layout that a GPU backend wants.
//! A material without state takes zero bytes. The internal dofs of the elements
//! (`C3D8I` mode amplitudes) are kept only when the store is made with `internal`.

use crate::SolverError;

/// A copy of the committed state, for a restart file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StateImage {
    pub ip: Vec<f64>,
    pub internal: Vec<f64>,
}

pub struct StateStore {
    /// First number of each element in `ip`, and one past the last at the end.
    start: Vec<usize>,
    ip: Vec<f64>,
    ip_trial: Vec<f64>,
    internal: Vec<[f64; 9]>,
    internal_trial: Vec<[f64; 9]>,
}

impl StateStore {
    /// `lens[e]` is the number of state numbers of element `e` (points times
    /// `Material::state_len`). `internal` keeps 9 numbers of internal dofs for each element.
    pub fn new(lens: &[usize], internal: bool) -> Self {
        let mut start = Vec::with_capacity(lens.len() + 1);
        let mut total = 0;
        for &l in lens {
            start.push(total);
            total += l;
        }
        start.push(total);
        let n_int = if internal { lens.len() } else { 0 };
        StateStore {
            start,
            ip: vec![0.0; total],
            ip_trial: vec![0.0; total],
            internal: vec![[0.0; 9]; n_int],
            internal_trial: vec![[0.0; 9]; n_int],
        }
    }

    fn range(&self, e: usize) -> std::ops::Range<usize> {
        self.start[e]..self.start[e + 1]
    }

    pub fn committed(&self, e: usize) -> &[f64] {
        &self.ip[self.range(e)]
    }

    pub fn trial(&self, e: usize) -> &[f64] {
        &self.ip_trial[self.range(e)]
    }

    pub fn set_trial(&mut self, e: usize, values: &[f64]) {
        let r = self.range(e);
        self.ip_trial[r].copy_from_slice(values);
    }

    pub fn committed_internal(&self, e: usize) -> [f64; 9] {
        self.internal.get(e).copied().unwrap_or([0.0; 9])
    }

    pub fn set_trial_internal(&mut self, e: usize, values: [f64; 9]) {
        if let Some(t) = self.internal_trial.get_mut(e) {
            *t = values;
        }
    }

    pub fn has_internal(&self) -> bool {
        !self.internal.is_empty()
    }

    /// The trial copy becomes the committed copy.
    pub fn commit(&mut self) {
        self.ip.copy_from_slice(&self.ip_trial);
        self.internal.copy_from_slice(&self.internal_trial);
    }

    /// Drop the trial copy: it becomes the committed copy again.
    pub fn cutback(&mut self) {
        self.ip_trial.copy_from_slice(&self.ip);
        self.internal_trial.copy_from_slice(&self.internal);
    }

    /// Bytes of both copies.
    pub fn bytes(&self) -> usize {
        2 * (self.ip.len() * 8 + self.internal.len() * 72)
    }

    /// The committed state, for a restart file.
    pub fn image(&self) -> StateImage {
        StateImage {
            ip: self.ip.clone(),
            internal: self.internal.iter().flatten().copied().collect(),
        }
    }

    /// Load a committed state. The trial copy gets the same values.
    pub fn load_image(&mut self, image: &StateImage) -> Result<(), SolverError> {
        if image.ip.len() != self.ip.len() || image.internal.len() != 9 * self.internal.len() {
            return Err(SolverError::new(
                "the restart file does not fit the model: the state sizes differ",
            ));
        }
        self.ip.copy_from_slice(&image.ip);
        for (dst, src) in self
            .internal
            .iter_mut()
            .zip(image.internal.as_chunks::<9>().0)
        {
            *dst = *src;
        }
        self.cutback();
        Ok(())
    }
}
