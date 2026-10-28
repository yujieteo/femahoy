# grid-free-block-modes-ccx derivation

This case checks the natural frequencies of a free body with the modes step of the built-in grid solver (units SI-mm). The body has no support, so its first 6 modes are rigid motions with a frequency of 0. The solver must find them and the elastic modes after them. Its shift is below 0, so the factor of K − σ M exists also for this singular K.

## Model

| Item | Value |
|---|---|
| Block | 60 mm in x, 40 mm in y, 10 mm in z |
| Steel | E = 210000 MPa, ν = 0.3, ρ = 7.85e-9 t/mm³ |
| Support | none |
| Step | `type = "modes"`, 12 modes |

## Reference

There is no closed form for the modes of a free block. The reference is CalculiX 2.23 on the same problem: `ccx/mesh.py` writes a structured grid of C3D20 elements (full integration) and a `*FREQUENCY` step of 12 modes, and `ccx/result.py` prints the frequencies from `job.dat`.

| Mode | H = 5 mm | H = 2.5 mm | H = 1.25 mm (reference) |
|---:|---:|---:|---:|
| 7 | 11977.06 | 11962.57 | 11961.42 |
| 8 | 13536.06 | 13530.45 | 13530.06 |
| 9 | 25642.70 | 25599.11 | 25595.73 |
| 10 | 28872.13 | 28843.68 | 28841.65 |
| 11 | 31576.33 | 31574.61 | 31574.50 |
| 12 | 31683.78 | 31637.27 | 31633.88 |

The mesh of H = 1.25 mm has 12,288 elements and 55,857 nodes. The largest change from H = 2.5 mm is 0.013 % (mode 9). CalculiX gives modes 1 to 6 below 0.011 Hz.

## Mesh study of the grid solver

| max_cell (mm) | unknowns | Mode 7 | Mode 9 | Mode 12 |
|---:|---:|---:|---:|---:|
| 5 | 1,053 | 12243.89 | 26481.60 | 32876.04 |
| 2.5 | 6,375 | 12041.10 | 25841.63 | 31970.65 |
| 1.25 | 43,659 | 11981.97 | 25658.88 | 31719.77 |

The changes fall by a factor of 3.4 to 3.6 for each halving: second order. The extrapolation of the 2 finer grids agrees with CalculiX to 0.04 % for each of these modes. At max_cell 1.25 mm, the errors against the reference are +0.17 % (mode 7), +0.10 % (8), +0.25 % (9), +0.21 % (10), +0.06 % (11) and +0.27 % (12). The grid solver gives modes 1 to 6 at 0 Hz: their eigenvalues are below 1e-10 of the mean of K_ii / M_ii.

The test `a_free_box_has_6_rigid_modes` in `crates/fem/tests/modes.rs` checks the rigid modes with both linear solvers: each eigenvalue is below 1e-6 of the first elastic eigenvalue, and the effective masses of the 6 rigid modes sum to the mass along x, y and z.

## Tolerance

The largest error of the grid solver at max_cell 1.25 mm is 0.27 %, and the reference changes by at most 0.013 %. Their sum is 0.29 %. The tolerance is 0.5 %.
