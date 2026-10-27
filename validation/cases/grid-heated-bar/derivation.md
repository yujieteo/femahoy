# grid-heated-bar derivation

This case checks the steady centre temperature of a square bar with a uniform heat source and its 4 long sides at 0 °C (units SI-m). It also measures the convergence of the grid solver.

## Source

The temperature solves k ∇²T = −q on the square of side a with T = 0 on the edges. This is the Poisson problem of the Prandtl stress function of a bar in torsion: S. P. Timoshenko and J. N. Goodier, *Theory of Elasticity*, 3rd ed., McGraw-Hill, 1970, section 109 (the torsion of a rectangular bar).
The series solution at the centre is

T(centre) = (q a² / k) · (1/8 − (4/π³) Σ_{n = 1, 3, 5, ...} (−1)^((n−1)/2) / (n³ cosh(nπ/2))) = 0.0736713533 · q a² / k.

200 terms give the coefficient to 1e-16.

## Model

| Item | Value |
|---|---|
| Bar | x 0 to 0.0025 m, y and z 0 to 0.02 m (a = 0.02 m) |
| Steel | k = 20 W/(m K) |
| Source | q = 1e7 W/m3 |
| Sides y−, y+, z−, z+ | T = 0 °C |
| Ends x− and x+ | adiabatic, so the field is the 2D field of the section |
| Symmetry | y and z: the solver solves 1 quarter |

q a² / k = 200 K. Thus T(centre) = 14.734270656 °C.

## Discretization error and tolerance

| max_cell (m) | T(centre) (°C) | Error |
|---:|---:|---:|
| 0.00125 | 14.779861 | +0.309 % |
| 0.000625 | 14.745623 | +0.077 % |

The error falls by a factor of 4.0 when the cell halves: the solver converges at second order, as the theory of trilinear elements gives. The Richardson extrapolation of the 2 grids gives 14.73421 °C, 4e-6 of the exact value.
The check uses max_cell 0.000625 m. Its error is 0.077 %. The tolerance is 0.1 %.
