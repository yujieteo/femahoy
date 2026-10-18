# Manufactured solution, C3D10

Domain: the unit cube `0 <= x, y, z <= 1`. Material: `E = 1000`, `nu = 0.3` (any values give the same field).

## Exact field

Let `a = 1.5` and `c = sqrt(2) a`. Let `phi = sin(a x) sin(a y) sinh(c z)`.
Then the Laplacian of `phi` is `-a^2 phi - a^2 phi + c^2 phi = 0`, so `phi` is harmonic.

The displacement `u = grad(phi)` has these parts:

- `u1 = a cos(a x) sin(a y) sinh(c z)`
- `u2 = a sin(a x) cos(a y) sinh(c z)`
- `u3 = c sin(a x) sin(a y) cosh(c z)`

`u` is the gradient of a harmonic function, so `div u = 0` and `curl u = 0`.
The Navier equation is `mu laplace(u) + (lambda + mu) grad(div u) = 0`. Here `laplace(u) = grad(laplace(phi)) = 0`
and `div u = 0`, so the equation holds with no body force.

## Boundary values and error

The harness reads the node set `BOUNDARY` (every node on a face of the cube) and fixes the 3 displacements of each
node to the exact field at the node. The step is added by the harness, because the mesh files hold no step.

The error of a mesh is the root mean square, over all nodes, of `|u_h - u|`. The rate is the slope of
`log(error)` against `log(h)` over the 3 meshes, with `h = 1/n` for `n` cells on each side.

## Meshes

`scripts/gen-mms-meshes.py` writes the meshes. The lattice points move by a smooth map that vanishes on the faces
(`jitter = 0.08`), so the cells are not parallelepipeds. The `C3D10` meshes split each cube into 6 tetrahedra with
straight edges. The `C3D8` and `C3D8I` meshes hold the same hexahedra and differ in the element type only.

The native solver had a test with the same field and the same meshes (`crates/core-solver/tests/convergence.rs` at main
commit `08a97c2`). The owner removed that solver on 2026-10-19. `fh validate` now solves the meshes with `fh-solve-ccx`.
