//! The linear solver of a model: the sparse Cholesky factor for a small model, and conjugate
//! gradients with the multigrid preconditioner for a large one. The solver is made once and
//! solves many right sides, for example each time step of a transient solve.

use crate::cholesky::Cholesky;
use crate::multigrid::{pcg, Level, Multigrid};
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

/// The relative residual of a PCG solve.
pub const PCG_TOL: f64 = 1e-10;
const PCG_MAX_ITER: usize = 500;

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
    /// `level.points[k]`. `method` forces a method, else the size and `many` (the solver solves
    /// many right sides) choose it.
    pub fn new(a: Csr, level: Level, method: Option<Method>, many: bool) -> Result<Solver, String> {
        let limit = if many {
            TRANSIENT_DIRECT_LIMIT
        } else {
            STEADY_DIRECT_LIMIT
        };
        let method = method.unwrap_or(if a.n <= limit {
            Method::Direct
        } else {
            Method::Multigrid
        });
        let inner = match method {
            Method::Direct => {
                let xyz: Vec<[f64; 3]> = level
                    .points
                    .iter()
                    .map(|p| std::array::from_fn(|d| level.planes[d][p[d] as usize]))
                    .collect();
                let sym = Symbolic::analyse(&a, &nested_dissection(&a, &xyz));
                let chol = Cholesky::factor(&sym, &a)?;
                Inner::Direct(a, chol)
            }
            Method::Multigrid => {
                let mg = Multigrid::new(a.clone(), level)?;
                Inner::Multigrid(a, mg)
            }
        };
        Ok(Solver { inner })
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
                    PCG_MAX_ITER,
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
