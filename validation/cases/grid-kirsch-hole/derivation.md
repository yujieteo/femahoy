# grid-kirsch-hole derivation

This case checks the stress concentration at a round hole on a mesh of 10-node tetrahedra. The plate is a polygon, so the solver meshes it with tetrahedra (`fh guide grid`). The reference is the Kirsch solution of an infinite plate with a round hole under a uniform tension (units SI-mm).

## Model

| Item | Value |
|---|---|
| Plate | the quarter x 0 to 40 mm, y 0 to 40 mm, z 0 to 1 mm |
| Hole | radius a = 1 mm, centre (0, 0): the arc is 16 straight segments between points on the circle |
| Material | E = 200000 MPa, ν = 0 |
| Cut plane x = 0 | ux = 0 (symmetry) |
| Cut plane y = 0 | uy = 0 (symmetry) |
| Bottom z = 0 | uz = 0 |
| End x = 40 mm | traction 100 MPa in x |

With ν = 0, the stress has no change through the thickness, and the 3D solution is the plane solution. The 16 segments of the quarter are the 64 segments that the solver makes of a full circle of this size.

## Reference

The Kirsch solution gives the hoop stress on the wall of the hole of an infinite plate under the tension σ along x: σθθ = σ (1 − 2 cos 2θ), with θ from the x axis (S. P. Timoshenko and J. N. Goodier, Theory of Elasticity, 3rd ed., McGraw-Hill 1970, section 35). At the top of the hole, θ = 90°: σθθ = σxx = 3 σ = 300 MPa. At its side, θ = 0: σθθ = σyy = −σ = −100 MPa. The probes `top` (0, 1, 0.5) and `side` (1, 0, 0.5) are points of the arc.

## Studies

Mesh study, 16 segments, plate 40 mm:

| max_cell (mm) | unknowns | top.sxx (MPa) | side.syy (MPa) |
|---:|---:|---:|---:|
| 4 | 21,710 | 301.222 | −99.575 |
| 2 | 54,648 | 301.171 | −99.580 |
| 1 | 190,368 | 301.182 | −99.589 |

The cells at the hole have the size of the segments for each max_cell, so the mesh error is below 0.02 %.

Arc study, max_cell 4 mm, plate 40 mm:

| segments in the quarter | top.sxx (MPa) | side.syy (MPa) |
|---:|---:|---:|
| 16 | 301.222 | −99.575 |
| 32 | 300.715 | −100.102 |
| 64 | 300.423 | −100.325 |

Width study, 16 segments, max_cell 4 mm: the plate of 80 mm gives top.sxx = 300.770 MPa, 0.45 MPa less than the plate of 40 mm. The effect of the width falls with the square of the width, so the plate of 40 mm is about 0.6 MPa (0.2 %) above the infinite plate.

## Tolerance

The polygon of the hole gives +0.4 % at the top and −0.4 % at the side against a true circle, the finite width +0.2 %, and the mesh less than 0.02 %. Their sum is 0.6 %, and the tolerance is 1 %. The solve takes about 1 s.
