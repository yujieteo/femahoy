# grid-frequency-cantilever derivation

This case checks the first natural frequency of a clamped steel beam with the modes step of the built-in grid solver (units SI-mm). It is the problem of the case `frequency-cantilever`, which uses gmsh and CalculiX.

## Source

R. D. Blevins, *Formulas for Natural Frequency and Mode Shape*, Van Nostrand Reinhold, New York, 1979, Table 8-1 (single-span beams), the clamped-free beam.
For mode 1 the table gives λ1 = 1.87510407 and f1 = λ1² / (2π L²) · sqrt(E I / m), with m the mass per length.

## Model

| Item | Value |
|---|---|
| Beam | 100 mm in x, 10 mm in y, 5 mm in z |
| Steel | E = 210000 MPa, ν = 0.3, ρ = 7.85e-9 t/mm³ |
| Support | the face x = 0: ux = uy = uz = 0 |
| Step | `type = "modes"`, 1 mode |

Mode 1 bends the beam in z, about the weaker axis: I = 10 · 5³ / 12 = 104.17 mm⁴. The mass per length is m = 7.85e-9 · 50 = 3.925e-7 t/mm.
Thus f1 = 1.87510407² / (2π · 100²) · sqrt(210000 · 104.17 / 3.925e-7) = 417.758 Hz.

## Mesh study

The cells are the 8-node hex with incompatible modes. The mass is the consistent mass of the 8 corners; the incompatible modes have no mass.

| max_cell (mm) | unknowns | f1 (Hz) | Difference |
|---:|---:|---:|---:|
| 5 | 540 | 420.713 | +0.71 % |
| 2.5 | 1,800 | 419.521 | +0.42 % |
| 1.25 | 10,800 | 419.088 | +0.32 % |
| 0.625 | 73,440 | 418.875 | +0.27 % |

The changes fall by a factor of 2.8, then 2.0. The extrapolation with the factor 2 gives 418.66 Hz, 0.22 % above the beam theory. The case `frequency-cantilever` found the same difference for C3D20R elements: the beam theory has no shear deformation, no rotary inertia and no clamp of the Poisson contraction. The error of the grid at max_cell 2.5 mm is 0.21 %.

The effective mass of mode 1 is 0.611 of the mass of the beam, against 0.613 for the beam theory.

The mass of the corners only is the standard practice for the incompatible modes. The mesh study confirms it: the frequency converges from above to the value of C3D20R, as a consistent mass with a conforming stiffness does.

## Tolerance

The sum of the 0.22 % of the beam theory and the 0.21 % of the discretization is 0.43 %. The tolerance is 1 %.
