# Kirsch plate with a hole: derivation

Input meshes: `mesh-4x4.inp`, `mesh-8x8.inp`, `mesh-16x16.inp`.
The names give the cells over 90 degrees and the cells in radius.
They are our own output of `scripts/cad/kirsch_plate.py`. Mesh ratio 2. Element type C3D10.

## Data

    a = 10 mm (hole radius), W = 200 mm (half-width of the square plate), t = pi a / (2 n) (one cell thick)
    sigma = 100 MPa tension along x on the face x = W
    E = 210000 MPa, nu = 0.3

## Model

- Quarter plate. Symmetry: u_x = 0 on x = 0, u_y = 0 on y = 0.
- Plane strain: u_z = 0 on all nodes.
- Load: pressure of -100 MPa (tension) with `*DLOAD` on the tet faces on x = W.
- The hole is a true circle. The midside nodes lie on it.
- Radial grading: s = (exp(2.5 x) - 1) / (exp(2.5) - 1), so the cells grow away from the hole.

## Closed form (infinite plate, uniaxial tension sigma along x, theta from the x axis)

    sigma_tt(a, theta) = sigma (1 - 2 cos(2 theta))

    theta = 90 degrees: sigma_tt = 3 sigma = 300 MPa (S11 at the point (0, a))
    theta = 0:          sigma_tt = -sigma = -100 MPa (S22 at the point (a, 0))

The closed form does not depend on nu for a traction boundary condition.

## Reference error

The plate is finite. The hole-edge stress of a finite-width strip is about 3 sigma times
(1 + 0.3 percent) at a / W = 0.05 (tables of Howland and Heywood). The case sets reference_error to 1 percent.
This value is an engineering estimate and needs the leader to approve it.

## Measured quantity

The solver gives one stress for each tetrahedron. The case takes the 4 tetrahedra on the hole
in the cell next to theta = 90 degrees (`edge90`, maximum S11) and in the cell next to theta = 0 (`edge0`, minimum S22).
The value is at the element centroid, so it is below the edge value and converges to it as the mesh gets finer.
