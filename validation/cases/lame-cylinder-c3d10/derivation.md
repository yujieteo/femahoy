# Lame thick cylinder with C3D10: derivation

Input mesh: `mesh-8x4.inp`, 8 cells in theta and 4 cells in radius.
It is our own output of a script that divides each hex cell into 24 quadratic tetrahedra.
The model is a quarter ring, 1 cell thick in z.

## Data

    a = 100 mm (inner radius), b = 200 mm (outer radius), t = 10 mm
    p = 100 MPa internal pressure, outer surface free
    E = 210000 MPa, nu = 0.3

## Model

- Symmetry: u_x = 0 on the plane x = 0. u_y = 0 on the plane y = 0.
- Plane strain: u_z = 0 on all nodes.
- Load: pressure p on the inner tet faces with `*DLOAD`.
- The inner and outer radii are straight chords between the cell corners.

## Closed form (plane strain, internal pressure only)

    A = p a^2 / (b^2 - a^2) = 100 * 10000 / 30000 = 33.33333 MPa
    B = p a^2 b^2 / (b^2 - a^2) = 100 * 10000 * 40000 / 30000 = 1333333.3 MPa mm^2

    s_r(r)     = A - B / r^2
    s_theta(r) = A + B / r^2
    u_r(r)     = (1 + nu) / E * ((1 - 2 nu) A r + B / r)

Check of the boundary conditions: s_r(a) = 33.333 - 133.333 = -100 MPa (equals -p). s_r(b) = 33.333 - 33.333 = 0.

## Reference values

    u_r(100) = 1.3 / 210000 * (0.4 * 33.33333 * 100 + 13333.333)
             = 6.190476e-6 * (1333.333 + 13333.333) = 0.0907937 mm
    u_r(200) = 6.190476e-6 * (0.4 * 33.33333 * 200 + 6666.667)
             = 6.190476e-6 * (2666.667 + 6666.667) = 0.0577778 mm

## Tolerance

`ccx` gave these values on 4 meshes (2026-10-19):

| Cells | u_r(100) (mm) | Difference | u_r(200) (mm) | Difference |
|---|---|---|---|---|
| 4 x 2 | 0.088350 | 2.7 percent | 0.056816 | 1.7 percent |
| 8 x 4 | 0.090185 | 0.7 percent | 0.057502 | 0.5 percent |
| 16 x 8 | 0.090642 | 0.2 percent | 0.057703 | 0.1 percent |
| 32 x 16 | 0.090756 | 0.04 percent | 0.057758 | 0.03 percent |

The difference falls with the mesh size, so the values converge to the closed form.
The case keeps only the 8 x 4 mesh, with a tolerance of 1 percent.
