# bar-pressure derivation

This derivation is for the cases `bar-pressure` (units SI-mm) and `bar-pressure-m` (units SI-m).
Both cases use the same part and the same load. Only the units are different.

## Model

A bar of 100 x 20 x 10 mm comes from `scripts/cad/bar.py`. The material has E = 210000 MPa (2.1e11 Pa) and nu = 0.3.
The case declares 4 named regions. `fh mesh` makes a gmsh physical surface for each region:

| Region | Face | Use |
|---|---|---|
| `SUPPORT` | x = 0 | ux = 0 |
| `SYM_Y` | y = 0 | uy = 0 |
| `SYM_Z` | z = 0 | uz = 0 |
| `LOAD` | x = 100 mm (0.1 m) | pressure p = -100 MPa (-1e8 Pa) |

A negative pressure pulls the face. The 3 supports remove all rigid motion. They do not stop the Poisson contraction.

## Solution

The stress field is uniform: sxx = -p = 100 MPa, and the other components are 0.
This field satisfies equilibrium and all boundary conditions:

- On x = 100, the traction is sxx = 100 MPa along +x. This is the traction of the pressure -100 MPa.
- On y = 20 and z = 10, the traction is 0.

The displacement is ux = (sxx / E) x, uy = -nu (sxx / E) y, uz = -nu (sxx / E) z.
It is linear, so C3D10 (quadratic) reproduces it to round-off. The consistent nodal forces of the pressure on a flat face give the exact total force.

| Quantity | SI-mm | SI-m |
|---|---:|---:|
| `s_xx`: mean sxx over the solid | 100 MPa | 1.0e8 Pa |
| `rf_x`: sum of the reactions on `SUPPORT` | -20000 N | -20000 N |
| `u_x_tip`: mean ux on `LOAD`, (sxx / E) x 100 mm | 0.047619047619 mm | 4.7619047619e-5 m |

The area of the face is 20 x 10 = 200 mm2 (2e-4 m2). The total force is 100 MPa x 200 mm2 = 20000 N.

## Files

`bar.step` comes from `python scripts/cad/bar.py bar.step` (mm) or `python scripts/cad/bar.py bar.step m` (m).
`bar.inp` comes from `fh mesh bar.step --case . --size H -o bar.msh` and `fh model bar.msh --case . -o bar.inp`.
H is 8 (mm) or 0.008 (m).
