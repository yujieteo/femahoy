# Cantilever with C3D10: derivation

Input mesh: `mesh-8.inp`, 8 cells along x and 1 cell across.
It is our own output of a script that divides each hex cell into 24 quadratic tetrahedra.

## Data

    L = 10 mm, b = 1 mm (y), h = 1 mm (z), A = b h = 1 mm^2
    E = 210000 MPa, nu = 0.3
    P = 1.0 N in -z on the 4 tip corner nodes (fixed end at x = 0)
    I = b h^3 / 12 = 1/12 mm^4
    G = E / (2 (1 + nu)) = 80769.23 MPa
    k = 5/6 (shear factor, rectangular section)

## Mesh

- Each hex face has a shared centre node and 4 triangles.
- Each triangle and the hex centre make a tetrahedron.
- All tetrahedra share their midside nodes on common edges.
- The clamp set holds all nodes on x = 0.
- The tip set holds all nodes on the tip face, so the measurement is the mean U3 over the tip face.

## Reference

Bending (Euler-Bernoulli):

    d_b = P L^3 / (3 E I) = 1.0 * 1000 / (3 * 210000 / 12) = 0.019047619 mm

Shear (Timoshenko):

    d_s = P L / (k G A) = 10 / (0.83333 * 80769.23) = 1.485714e-4 mm

Total:

    d = d_b + d_s = 0.019196190 mm in -z

The reference value in `check.toml` is -0.01919619048 mm (z is the load direction).

## Tolerance

Beam theory ignores the 3D stress state at the clamp. The case sets 2 percent of the reference as the stated error.
This value is an engineering estimate, not a derived bound. The captain accepted it.

`ccx` gave these values on 4 meshes (2026-10-19):

| Cells along x | Tip deflection (mm) | Difference from the reference |
|---|---|---|
| 4 | -0.018662 | 2.8 percent |
| 8 | -0.018923 | 1.4 percent |
| 16 | -0.019006 | 1.0 percent |
| 32 | -0.019027 | 0.9 percent |

The case keeps only the 8-cell mesh. It is the coarsest mesh in the 2 percent tolerance.
