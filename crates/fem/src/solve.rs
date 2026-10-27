//! The linear solver of a model: the sparse Cholesky factor for a small model, and conjugate
//! gradients with the multigrid preconditioner for a large one. The solver is made once and
//! solves many right sides, for example each time step of a transient solve.

use crate::cholesky::Cholesky;
use crate::multigrid::{pcg, Cells, Level, Multigrid};
use crate::order::nested_dissection;
use crate::sparse::Csr;
use crate::symbolic::Symbolic;

/// Up to this many unknowns, a solver for 1 right side is direct. Reason: the ignored test
/// `crossover` (Apple M-series, 1 thread) measured the direct solver at 0.008 s and multigrid
/// PCG at 0.007 s on a cube of 1,210 unknowns, and 0.052 s against 0.028 s at 4,624. Above that,
/// the factor grows as n^2 in time on a cube (21 s against 1.7 s at 130,050 unknowns).
pub const STEADY_DIRECT_LIMIT: usize = 3_000;

/// Up to this many unknowns, a solver for many right sides (time steps) is direct, because each
/// step after the factor costs only 2 triangular solves. Reason: the `crossover` laminate with
/// 100 steps took 10.3 s direct against 15.2 s multigrid at 28,349 unknowns, and 36.2 s against
/// 25.7 s at 61,749.
pub const TRANSIENT_DIRECT_LIMIT: usize = 40_000;

/// How a solver chooses its method when the model does not force one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Crossover {
    /// Direct up to this many unknowns.
    Unknowns(usize),
    /// Direct when the factor has up to `entries` entries, and it costs up to `flops` for each
    /// unknown or the cells are `flat`. The factor of a thin model is cheap for its size, and
    /// multigrid converges slowly on its flat cells.
    Flops {
        flops: f64,
        entries: u64,
        flat: bool,
    },
}

/// The relative residual of a PCG solve.
pub const PCG_TOL: f64 = 1e-10;
pub const PCG_MAX_ITER: usize = 500;

/// How a solver solves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Direct,
    Multigrid,
}

enum Inner {
    Direct(Csr, Cholesky),
    Multigrid(Csr, Multigrid),
}

pub struct Solver {
    inner: Inner,
    max_iter: usize,
}

/// The work of 1 solve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// The PCG iterations, 0 for the direct solver.
    pub iterations: usize,
    /// |b - A x| / |b|.
    pub residual: f64,
}

impl Solver {
    /// The solver of `a` (symmetric positive definite) whose unknown `k` is the grid point
    /// `level.points[k]`. `method` forces a method, else `crossover` chooses it. `cells` gives the
    /// coarse matrices of a displacement to multigrid.
    pub fn new(
        a: Csr,
        level: Level,
        method: Option<Method>,
        crossover: Crossover,
        cells: Option<Cells>,
    ) -> Result<Solver, String> {
        let symbolic = |a: &Csr, level: &Level| {
            let xyz: Vec<[f64; 3]> = level
                .points
                .iter()
                .map(|p| std::array::from_fn(|d| level.planes[d][p[d] as usize]))
                .collect();
            Symbolic::analyse(a, &nested_dissection(a, &xyz))
        };
        let sym = match (method, crossover) {
            (Some(Method::Direct), _) => Some(symbolic(&a, &level)),
            (Some(Method::Multigrid), _) => None,
            (None, Crossover::Unknowns(n)) => (a.n <= n).then(|| symbolic(&a, &level)),
            (
                None,
                Crossover::Flops {
                    flops,
                    entries,
                    flat,
                },
            ) => {
                let sym = symbolic(&a, &level);
                (sym.entries() <= entries && (flat || sym.flops() <= flops * a.n as f64))
                    .then_some(sym)
            }
        };
        let inner = match sym {
            Some(sym) => {
                let chol = Cholesky::factor(&sym, &a)?;
                Inner::Direct(a, chol)
            }
            None => {
                let mg = Multigrid::new(a.clone(), level, cells)?;
                Inner::Multigrid(a, mg)
            }
        };
        Ok(Solver {
            inner,
            max_iter: PCG_MAX_ITER,
        })
    }

    /// The solver with at most `max_iter` PCG iterations for each solve.
    pub fn with_max_iter(self, max_iter: usize) -> Solver {
        Solver { max_iter, ..self }
    }

    pub fn method(&self) -> Method {
        match self.inner {
            Inner::Direct(..) => Method::Direct,
            Inner::Multigrid(..) => Method::Multigrid,
        }
    }

    /// Solve A x = b. `x` holds a start value for PCG and gets the solution.
    pub fn solve(
        &self,
        b: &[f64],
        x: &mut [f64],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Stats, String> {
        match &self.inner {
            Inner::Direct(a, chol) => {
                x.copy_from_slice(b);
                chol.solve(x);
                let mut ax = vec![0.0; b.len()];
                a.mul(x, &mut ax);
                let norm = |v: &mut dyn Iterator<Item = f64>| v.map(|e| e * e).sum::<f64>().sqrt();
                let bn = norm(&mut b.iter().copied());
                let rn = norm(&mut b.iter().zip(&ax).map(|(bi, ai)| bi - ai));
                Ok(Stats {
                    iterations: 0,
                    residual: if bn > 0.0 { rn / bn } else { rn },
                })
            }
            Inner::Multigrid(a, mg) => {
                let c = pcg(
                    a,
                    b,
                    x,
                    &|r, z| mg.apply(r, z),
                    PCG_TOL,
                    self.max_iter,
                    cancelled,
                )?;
                Ok(Stats {
                    iterations: c.iterations,
                    residual: c.residual,
                })
            }
        }
    }
}
