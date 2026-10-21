# naca0012-ladson derivation

This case checks the CFD path: an OpenFOAM case through `fh validate`, `simpleFoam` with the Spalart-Allmaras (SA) model, and the force coefficients of `forceCoeffs`.
It is the plain NACA 0012 airfoil, before a case with a flap.

## Sources

- C. L. Ladson, "Effects of Independent Variation of Mach and Reynolds Numbers on the Low-Speed Aerodynamic Characteristics of the NACA 0012 Airfoil Section", NASA TM-4074, October 1988, https://ntrs.nasa.gov/citations/19880019495.
  The test was in the Langley Low-Turbulence Pressure Tunnel, with a chord of 0.601 m. The lift comes from the surface pressures, and the drag from a wake rake. Grit at 5 % of the chord fixed the transition.
- The check values come from Table IX on page 19: M = 0.15, transition fixed with No. 80-W grit, R = 5.95 x 10^6.
- NASA Langley Turbulence Modeling Resource (TMR), "2D NACA 0012 Airfoil Validation Case" (2DN00), https://turbmodels.larc.nasa.gov/naca0012_val.html.
  It gives the airfoil with a sharp trailing edge, Re = 6 million and M = 0.15. Its file "Ladson force data" has the same values as Table IX. TMR tells that these tripped data are the most applicable data for the forces of fully turbulent CFD at Re = 6 million.
- TMR, "SA Model Results", https://turbmodels.larc.nasa.gov/naca0012_val_sa.html. It gives the SA forces of 7 CFD codes on the TMR grid of 897 x 257 points.

| Angle | Lift cl | Drag cd | Check |
|---:|---:|---:|---|
| -0.05 | -0.0126 | 0.00809 | `cd_a0` |
| 4.04 | 0.4316 | 0.00823 | `cl_a4`, `cd_a4` |
| 10.12 | 1.0707 | 0.01201 | `cl_a10`, `cd_a10` |

The lift at -0.05 degrees is not a check. The lift slope of Table IX (0.107 per degree) gives -0.005 there. Thus -0.0126 is an offset of the test, not a property of the airfoil.

## Model

`python3 validation/airfoil.py naca0012-ladson` writes the 3 OpenFOAM cases `a-0.05`, `a4.04` and `a10.12`. They differ only in the angle of the flow and of the force directions.

| Item | Value |
|---|---|
| Airfoil | TMR formula with a sharp trailing edge, chord 1, span 1 (1 cell, `empty` front and back) |
| Flow | incompressible and steady, `simpleFoam` with `consistent yes`, relaxation 0.9 for U and nuTilda |
| Speed and viscosity | \|U\| = 1, nu = 1/6e6, thus Re = 6 million |
| Turbulence | `SpalartAllmaras` of OpenFOAM v2512, fully turbulent. nuTilda = 3 nu in the free stream (as TMR), 0 on the wall |
| Far field | `freestreamVelocity`, `freestreamPressure` and `freestream`, about 500 chords from the airfoil (as TMR) |
| Wall | `noSlip`, `nutLowReWallFunction` |
| Schemes | `linearUpwind` for U and nuTilda, limited gradients |
| Forces | `forceCoeffs` on the wall with lRef = 1, Aref = 1, magUInf = 1. The lift and drag directions turn with the flow |
| Iterations | 1500 |

## Mesh

The mesh is a C-grid of 12 hexahedral blocks from `blockMesh`. The inner blocks are 1 chord thick, and their lines leave the wall along the normal. The outer blocks go to a circle of 500 chords in front, and to straight lines at y = +/-500 above and below the wake. The outlet is at x = 500.
Each level doubles the cells in each direction and halves the first cell height.

| Level | Cells | Cells on the wall | First cell height | y+ on the wall (mean, max) at 4.04 degrees | Largest non-orthogonality |
|---:|---:|---:|---:|---|---:|
| 1 | 9216 | 128 | 8e-6 | 0.90, 1.96 | 74.7 degrees |
| 2 (the check) | 36864 | 256 | 4e-6 | 0.45, 0.97 | 65.6 degrees |
| 3 | 147456 | 512 | 2e-6 | 0.22, 0.48 | 50.7 degrees |

`checkMesh` also reports cells with a high aspect ratio, up to 2e7. They are in the wake. There the cells keep the first cell height of the wall to the outlet, and their length grows to the outlet.

## Mesh study

These commands, in the repository root, write level 3 in a scratch folder and solve it at 4.04 degrees. Solve the other angles in the same way. Each level and angle starts from the free stream.

```
python3 validation/airfoil.py naca0012-ladson 3 6000 /tmp/naca-l3
openfoam blockMesh -case /tmp/naca-l3/a4.04
openfoam simpleFoam -case /tmp/naca-l3/a4.04
```

| Level | Iterations | cd at -0.05 | cl at 4.04 | cd at 4.04 | cl at 10.12 | cd at 10.12 |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 3000 | 0.011198 | 0.43949 | 0.011344 | 1.04486 | 0.015877 |
| 2 | 3000 | 0.009063 | 0.44477 | 0.009471 | 1.08222 | 0.013014 |
| 3 | 6000 | 0.008519 | 0.44556 | 0.008976 | 1.08782 | 0.012551 |
| Extrapolated | | 0.008334 | 0.44569 | 0.008798 | 1.08881 | 0.012461 |
| 2, the check | 1500 | 0.009126 | 0.44494 | 0.009500 | 1.08233 | 0.013030 |

Each extrapolated value comes from the 3 levels, with the observed order p of the convergence:
p = log2((f1 - f2) / (f2 - f3)), and f = f3 + (f3 - f2) / (2^p - 1). The value p is 1.9 to 2.8.
Thus the mesh error of level 2 is -0.2 % in lift at 4.04 degrees and -0.6 % at 10.12 degrees, and +5.5 to +7.3 drag counts.

The extrapolated values agree with the SA results of TMR:
- The lift at 10.12 degrees is 1.0888. With the Prandtl-Glauert factor 1.011 for M = 0.15, it is 1.101. CFL3D gives 1.0909 at 10 degrees, thus about 1.104 at 10.12 degrees.
- The drag at 10.12 degrees is 0.01246. CFL3D gives 0.01231 at 10 degrees, thus about 0.0124 at 10.12 degrees.
- The drag at -0.05 degrees is 0.00833. CFL3D gives 0.00819 at 0 degrees, and the 7 codes give 0.00812 to 0.00830.

At 1500 iterations, level 2 is within 0.7 % of its drag at 3000 iterations, and within 0.04 % of its lift.
The 3 cases of the check run at the same time. On a Mac with 10 cores, they took 171 s while 3 other solves ran.

## Tolerance

The tolerance is `rel_tol = 0.04` and `abs_tol = 0.0015` (15 drag counts, 1 count = 0.0001).
Thus a lift passes within 4.3 % at 4.04 degrees and 4.1 % at 10.12 degrees. A drag passes within 18 to 20 counts.
The tolerance holds these differences between the model and the test:

| Cause | Lift | Drag |
|---|---|---|
| SA against the test: the 7 codes of TMR against Table IX at 0 and 10 degrees (interpolated to 10.0 degrees: cl 1.0586, cd 0.01191) | +2.9 % to +3.9 % | +0.3 to +2.1 counts at 0, +3.4 to +5.4 counts at 10 |
| Spread of the test: Table IX (80-W grit) against Table XI (120 grit, R = 6.00 x 10^6, page 21) at the same angle | up to 0.8 % | up to 5.6 counts |
| Repeat points of the test (TM-4074, "Accuracy") | 0.004 in normal force | 2 counts |
| Mesh error of level 2 (section "Mesh study") | -0.2 % to -0.6 % | +5.5 to +7.3 counts |
| Iterations: 1500 against 3000 | 0.04 % | 0.6 counts |
| Incompressible flow against M = 0.15 (Prandtl-Glauert factor 1.011) | -1.1 % | small |
| Re = 6.00 against 5.95 million | small | small |

Level 2 at 1500 iterations is 3.1 % above the test in lift at 4.04 degrees and 1.1 % at 10.12 degrees. It is 10 to 13 counts above the test in drag.
A larger difference shows a fault in the setup or in `fh`, not a property of SA.
