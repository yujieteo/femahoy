# grid-waveguide-te10 derivation

This case checks the built-in electromagnetic solver on the simplest wave: the TE10 mode of an empty rectangular waveguide. The solver uses edge elements on tetrahedra and a TE10 wave port at each end (`fh guide grid`). The case is not a chip package problem. Units are SI-mm.

## Source

A rectangular waveguide of width a has the TE10 mode with the propagation constant β = sqrt(k0² − (π/a)²), k0 = 2π f / c. A uniform guide with matched ports has S11 = 0 and S21 = exp(−jβL) for the length L between the ports, with the time factor exp(jωt). D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley, 2012, section 3.3.

## Model

| Item | Value |
|---|---|
| Guide | WR-90, a = 22.86 mm, b = 10.16 mm, L = 30 mm along z, air |
| Walls | perfect conductors (the default wall) |
| Ports | TE10 at z = 0 (port 1) and z = 30 mm (port 2) |
| Frequency | 10 GHz: only TE10 propagates (TE10 cuts off at 6.557 GHz, TE20 at 13.11 GHz) |

β = sqrt(209.584502² − 137.428349²) = 158.238256 rad/m. βL = 4.747148 rad, so the phase of S21 is −271.991527 + 360 = 88.008473 degrees.

| Check | Formula | Value |
|---|---|---|
| `s21_mag` | \|exp(−jβL)\| | 1 |
| `s21_deg` | −βL in degrees, in (−180, 180] | 88.008473 |
| `s11_mag` | a matched uniform guide | 0 |

## Mesh study

| max_cell (mm) | points | unknowns | \|S11\| | \|S21\| | phase of S21 (degrees) | time |
|---:|---:|---:|---:|---:|---:|---:|
| 4 | 324 | 1,233 | 0.02039 | 0.998629 | 87.788380 | 0.01 s |
| 2 | 2,160 | 11,176 | 0.00623 | 0.999837 | 87.962842 | 0.17 s |
| 1 | 15,934 | 96,531 | 0.00140 | 0.999976 | 88.015033 | 10.9 s |

The error of the phase is −0.220, −0.046 and +0.0066 degrees, and \|S11\| falls by a factor of 3.3 and 4.5 when the cell halves: second order, as for linear edge elements. The port uses the exact β, so the reflection at a port shows the difference between the exact β and the β of the mesh.

## Tolerance

The check uses max_cell 2 mm. The pass rule is \|error\| <= rel_tol \|value\| + abs_tol, with rel_tol 1e-3 and abs_tol 0.012. This holds \|S11\| = 0.0062 with a margin of a factor of 1.9 and the phase error of 0.046 degrees in a band of 0.1 degrees.
