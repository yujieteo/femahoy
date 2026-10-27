# grid-pipe-wall derivation

This case checks the built-in solver on a shape that is not a box: a pipe wall, a circle with a round hole, extruded along z. The solver meshes it with tetrahedra (`fh guide grid`). The case is not a chip package problem. Units are SI-m.

## Source

Steady conduction through a cylinder wall with no source: the heat flow per length q' is the same through each radius, so T(r) = T1 − q' ln(r / r1) / (2π k). A convection outside adds the film resistance 1 / (2π r2 h) in series with the wall resistance ln(r2 / r1) / (2π k). F. P. Incropera, D. P. DeWitt, T. L. Bergman and A. S. Lavine, *Fundamentals of Heat and Mass Transfer*, 6th ed., Wiley, 2007, section 3.3.1 (the cylindrical wall).

## Model

| Item | Value |
|---|---|
| Wall | PVC, k = 0.2 W/(m K), r1 = 0.05 m, r2 = 0.08 m |
| Length | z 0 to 0.002 m, ends adiabatic, so the field is the 2D field of the section |
| Inner face, r = r1 | T1 = 80 °C (hot water) |
| Outer face, r = r2 | convection, h = 10 W/(m2 K), air at 20 °C |

Wall resistance: ln(1.6) / (2π · 0.2) = 0.374009 K m/W. Film resistance: 1 / (2π · 0.08 · 10) = 0.198944 K m/W. q' = 60 / 0.572953 = 104.719227 W/m.

| Check | Formula | Value |
|---|---|---|
| `middle`, r = 0.065 m on the x axis | T1 − q' ln(1.3) / (2π k) | 58.136421500 °C |
| `middle_45`, r = 0.065 m at 45 degrees | the same | 58.136421500 °C |
| `outer`, r = r2 | T1 − q' ln(1.6) / (2π k) | 40.833228321 °C |
| `heat_flow`, the heat in at the inner face | q' · 0.002 m | 0.209438455 W |

The 2 middle probes are at 2 angles, so the check also sees an error that depends on the angle.

## Mesh study

The solver puts the points of each circle on the circle, with segments of at most `max_cell`. Thus the faces are polygons, and the area of the outer face is a little too small. This error falls at second order, as the error of the linear tetrahedra does.

| max_cell (m) | points | middle (°C) | middle_45 (°C) | outer (°C) | heat_flow (W) | time |
|---:|---:|---:|---:|---:|---:|---:|
| 0.004 | 5,187 | 58.128937 | 58.138120 | 40.826734 | 0.20940330 | 0.02 s |
| 0.002 | 19,629 | 58.137421 | 58.135139 | 40.831908 | 0.20942950 | 0.07 s |
| 0.001 | 76,377 | 58.135673 | 58.136530 | 40.832828 | 0.20943607 | 0.33 s |

The error of the heat flow is −1.7e-4, −4.3e-5 and −1.1e-5 of the value: it falls by a factor of 4 when the cell halves. The errors at the probes are below 1.3e-3 °C at max_cell 0.002 m, and their sign changes, because a probe interpolates in a tetrahedron that is at a different place in each mesh.

## Tolerance

The check uses max_cell 0.002 m. Its largest relative error is 4.3e-5 (the heat flow). The tolerance is 1e-4 of each value.
