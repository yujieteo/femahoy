# bar-pressure derivation

This derivation is for the case `bar-pressure` (units SI-mm).

## Model

A bar of 100 x 20 x 10 mm comes from `scripts/cad/bar.py`. The material has E = 210000 MPa and nu = 0.3.
The input uses 4 named sets. `fh mesh --face` makes each set as a node set and a surface:

| Set | Face | Use |
|---|---|---|
| `SUPPORT` | x = 0 | ux = 0 |
| `SYM_Y` | y = 0 | uy = 0 |
| `SYM_Z` | z = 0 | uz = 0 |
| `LOAD` | x = 100 mm | pressure p = -100 MPa |

A negative pressure pulls the face. The 3 supports remove all rigid motion. They do not stop the Poisson contraction.

## Solution

The stress field is uniform: sxx = -p = 100 MPa, and the other components are 0.
This field satisfies equilibrium and all boundary conditions:

- On x = 100, the traction is sxx = 100 MPa along +x. This is the traction of the pressure -100 MPa.
- On y = 20 and z = 10, the traction is 0.

The displacement is ux = (sxx / E) x, uy = -nu (sxx / E) y, uz = -nu (sxx / E) z.
It is linear, so C3D10 (quadratic) reproduces it to round-off. The consistent nodal forces of the pressure on a flat face give the exact total force.

| Quantity | Value |
|---|---:|
| `s_xx`: mean sxx over the solid | 100 MPa |
| `rf_x`: sum of the reactions on `SUPPORT` | -20000 N |
| `u_x_tip`: mean ux on `LOAD`, (sxx / E) x 100 mm | 0.047619047619 mm |

The area of the face is 20 x 10 = 200 mm2. The total force is 100 MPa x 200 mm2 = 20000 N.

## Files

`bar.step` comes from `python scripts/cad/bar.py bar.step`.
The mesh in `job.inp` comes from `fh mesh bar.step --units SI-mm --size 8 --face SUPPORT=x:0 --face SYM_Y=y:0 --face SYM_Z=z:0 --face LOAD=x:100`.
