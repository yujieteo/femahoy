# grid-lame-cylinder derivation

This case checks the stress and the displacement of a thick tube under an internal pressure, on a mesh of 10-node tetrahedra. The tube is a `circle` part with a circle in `holes`, and the pressure is on the face set of the hole wall (`fh guide grid`). The reference is the Lamé solution (units SI-mm).

## Model

| Item | Value |
|---|---|
| Tube | inner radius a = 10 mm, outer radius b = 20 mm, z 0 to 2 mm, centre (0, 0) |
| Material | E = 210000 MPa, ν = 0.3 |
| Inner wall | pressure p = 100 MPa |
| Bottom z = 0 | uz = 0 |
| Point supports | uy = 0 at (±20, 0, 0), ux = 0 at (0, 20, 0) |

The ends are open: the top is free and the bottom holds only uz. The solution is symmetric, so the 3 point supports take no force. They only stop the rigid motion in the plane x y.

## Reference

The Lamé solution of a tube with open ends (S. P. Timoshenko and J. N. Goodier, Theory of Elasticity, 3rd ed., McGraw-Hill 1970, section 28): σrr = A − B / r², σθθ = A + B / r², σzz = 0, with A = p a² / (b² − a²) = 33.3333 MPa and B = p a² b² / (b² − a²) = 13333.33 MPa mm². The axial strain is constant, so the 3D solution of the short tube is exact. The radial displacement is u = ((1 − ν) A r + (1 + ν) B / r) / E.

| Probe | Point (mm) | Column | Reference |
|---|---|---|---:|
| `inner` | (10, 0, 1) | ux = u(10) | 9.365079e-3 mm |
| `wall` | (10.5, 0, 1) | syy = σθθ(10.5) | 154.2706 MPa |
| `middle` | (0, 15, 1) | uy = u(15) | 7.169312e-3 mm |
| `middle` | (0, 15, 1) | sxx = σθθ(15) | 92.59259 MPa |
| `outer` | (0, 20, 1) | uy = u(20) | 6.349206e-3 mm |
| `outer` | (0, 20, 1) | sxx = σθθ(20) | 66.66667 MPa |

## Mesh study

| max_cell (mm) | unknowns | inner.ux | wall.syy | middle.uy | middle.sxx | outer.uy | outer.sxx |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 2.5 | 28,641 | 9.35181e-3 | 154.146 | 7.16065e-3 | 92.509 | 6.34368e-3 | 66.624 |
| 1.25 | 71,677 | 9.34731e-3 | 153.962 | 7.15682e-3 | 92.429 | 6.33850e-3 | 66.256 |
| 0.8 | 243,997 | 9.35426e-3 | 154.034 | 7.16074e-3 | 92.484 | 6.34185e-3 | 66.589 |

The solver makes each circle a polygon of 64 points (80 at max_cell 0.8 mm). The displacements are 0.1 % to 0.2 % low, the effect of the polygon. The stresses scatter by up to 0.6 % with the mesh.

A point of a circle is a corner of the polygon. On the inner wall, which has the pressure, the corner is re-entrant by 5.6°, and its stress changes by a few percent with the mesh: `inner.syy` is 166.75, 166.74 and 174.79 MPa on the 3 meshes, against 166.67 MPa. For this reason the check reads the hoop stress of the inner wall at `wall`, 0.5 mm inside the wall, where it changes by less than 0.2 %.

## Tolerance

The polygons give about 0.2 %, and the mesh changes the values by at most 0.6 %. Their sum is 0.8 %, and the tolerance is 1 %. The solve takes about 2 s.
