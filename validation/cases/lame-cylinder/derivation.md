# Lame thick cylinder: derivation

Input meshes: `mesh-4x2.inp`, `mesh-8x4.inp`, `mesh-16x8.inp` (n_theta x n_r C3D8 elements). They are our own output of a mesh script.
Mesh ratio 2. The model is a quarter ring, one element thick in z.

## Data

    a = 100 mm (inner radius), b = 200 mm (outer radius), t = 10 mm
    p = 100 MPa internal pressure, outer surface free
    E = 210000 MPa, nu = 0.3

## Model

- Symmetry: u_x = 0 on the plane x = 0. u_y = 0 on the plane y = 0.
- Plane strain: u_z = 0 on all nodes.
- Load: pressure p on the inner surface, as nodal forces. Each chord face gets F = p * chord * t. The 4 face nodes get F/4 each, along the chord normal. The sum of the x forces is p a t = 100000 N.

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
