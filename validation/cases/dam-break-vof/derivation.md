# dam-break-vof derivation

This case checks the free surface path: a 2D dam break with the volume of fluid (VOF) method of OpenFOAM (`interFoam`).
A water column falls on a dry floor in air, and the surge front moves along the floor. The reference is a measurement. Thus the tolerance comes from the scatter of the measurement and from the mesh and time step studies of the model.

## Why this case

No other case of the suite has any of these items:

- 2 fluids, water and air, with a sharp interface between them: the VOF method with MULES and interface compression.
- A free surface with a large motion under gravity.
- A start field from `setFields`.
- A time step that changes with the Courant number of the flow and of the interface.
- A transient with a value from the time history: the time at which the front gets to a position.

## Source

J. C. Martin and W. J. Moyce, "Part IV. An experimental study of the collapse of liquid columns on a rigid horizontal plane", Phil. Trans. R. Soc. Lond. A 244 (1952) 312-324, https://doi.org/10.1098/rsta.1952.0006.

A column of water of width a and height n^2 a stands on a horizontal plane, against a wall at x = 0. At t = 0 the column is released. Table 2 (page 317) gives the times for n^2 = 2, a = 2.25 in, in the dimensionless form T = n t sqrt(g / a) = t sqrt(2 g / a) at the front positions Z = x / a.
The source sets T to 1.19 at Z = 1.44 in each run ("T normalized at this value of Z"), because the instant of the release is not known exactly. The model does the same: it adds a constant to its T so that T is 1.19 at Z = 1.44.

The checks use the column "mean" for a = 2.25 in. The single runs and their standard deviation come from the same table:

| Check | Z | Measured T (mean) | Single runs | Standard deviation of the runs |
|---|---:|---:|---:|---:|
| `T_211` | 2.11 | 1.83 | 9, from 1.75 to 1.88 | 0.048 (2.6 %) |
| `T_300` | 3.00 | 2.51 | 9, from 2.38 to 2.66 | 0.099 (3.9 %) |
| `T_389` | 3.89 | 3.11 | 4, from 2.94 to 3.25 | 0.139 (4.5 %) |

The 3 points are near the start, the middle and the end of the data that the source gives for most runs of a = 2.25 in.

## Model

The OpenFOAM case is the folder `n40`. The units are SI.

| Item | Value |
|---|---|
| Column | a = 0.05715 m, height 2a, water from x = 0 to a |
| Tank | 5a long and 2.5a high: walls at x = 0, at x = 5a and at y = 0, an open top. 1 cell in z |
| Mesh | uniform square cells, dx = a / 40 = 1.43 mm. 200 x 100 = 20000 cells. `n` in `system/blockMeshDict` sets the cells across a |
| Fluids | water: rho = 998 kg/m^3, nu = 1e-6 m^2/s. Air: rho = 1.2 kg/m^3, nu = 1.5e-5 m^2/s. Surface tension 0.073 N/m. Laminar |
| Solver | `interFoam` with PIMPLE: 1 outer corrector, 3 pressure correctors, no momentum predictor |
| Interface | MULES with interface compression `cAlpha 1`, `MULESCorr yes`, 2 corrections. `Gauss vanLeer` for `div(phi,alpha)` |
| Momentum | `Gauss linearUpwind grad(U)` for `div(rhoPhi,U)`, Euler in time |
| Time step | adjustable, with `maxCo` and `maxAlphaCo` 0.5. 369 steps to t = 0.18 s |
| Walls | `noSlip`, `fixedFluxPressure`, and `zeroGradient` for `alpha.water` (a contact angle of 90 degrees) |
| Top | `totalPressure` 0 for `p_rgh`, `pressureInletOutletVelocity` for `U`, `inletOutlet` for `alpha.water` |
| Start | at rest. `setFields` puts alpha.water = 1 in the cells of the column (`boxToCell`) |
| Front | the largest x of a cell centre with alpha.water > 0.5. The function object `front` (`fieldExtents`) writes it after each time step |
| Output | at the end, the function object `frontTimes` (`systemCall`) starts `awk -f system/front.awk`. It writes `front.txt`: 1 row with the 3 values of the check. It also writes the curve `front.csv` for `fh report --curve`: the front at each step and the measured points |

The run stops at t = 0.18 s. Then the front is at Z = 4.5, before the wall at x = 5a.

The front moves 1 cell at a time. Thus `front.awk` interpolates linearly between the 2 steps of the front on each side of x = Z a.
The largest cell centre is up to 1 cell behind the surface alpha.water = 0.5. This offset is almost the same at Z = 1.44 and at the 3 points of the check, thus the normalization removes most of it. The mesh study includes the remainder.

The front of the model gets to Z = 1.44 at T = 0.91: the release of the model is instantaneous. The constant that sets T to 1.19 is thus 0.28.

## Studies

These commands, in the repository root, solve the case with other values in a scratch folder. On Linux, use `openfoam2512` in place of `openfoam`:

```
cp -R validation/cases/dam-break-vof/n40 /tmp/dam && cd /tmp/dam
sed -i.orig 's/^n       40;/n       80;/' system/blockMeshDict
openfoam blockMesh && openfoam setFields && openfoam interFoam
```

The values are in `/tmp/dam/front.txt`.

Mesh, with `maxCo` and `maxAlphaCo` 0.5:

| n (cells across a) | Cells | T_211 | T_300 | T_389 |
|---:|---:|---:|---:|---:|
| 10 | 1250 | 1.8174 | 2.5479 | 3.2045 |
| 20 | 5000 | 1.8787 | 2.5897 | 3.2195 |
| 40, the check | 20000 | 1.8717 | 2.5856 | 3.2057 |
| 80 | 80000 | 1.8756 | 2.5926 | 3.2203 |
| Measured | | 1.83 | 2.51 | 3.11 |

From n = 20 to n = 80, the values change by 0.5 % or less, with no monotonic trend. `mesh.csv` holds the values of `T_389` as a curve for `fh report --curve`. Thus an extrapolation is not possible. The mesh error of n = 40 is the difference to n = 80: 0.0146 for `T_389` (0.47 % of the reference), 0.0070 for `T_300` (0.28 %) and 0.0039 for `T_211` (0.21 %).

Time step and walls, with n = 40. For these rows, edit `maxCo` and `maxAlphaCo` in `system/controlDict`, or the type of `walls` in `0/U`:

| Change | T_211 | T_300 | T_389 |
|---|---:|---:|---:|
| The check: `maxCo` and `maxAlphaCo` 0.5, `noSlip` | 1.8717 | 2.5856 | 3.2057 |
| `maxCo` and `maxAlphaCo` 0.25 (989 steps) | 1.8747 | 2.5883 | 3.2118 |
| `slip` walls | 1.8766 | 2.5883 | 3.2104 |

Half the Courant number changes the values by 0.2 % or less. The wall condition changes them by 0.3 % or less: the boundary layer of the floor is thin and has a small effect on the front.

## Tolerance

The tolerance is `rel_tol = 0.055` and `abs_tol = 0`.

| Cause | Basis | Effect |
|---|---|---:|
| The scatter of the measurement | The largest standard deviation of the single runs about the mean (at Z = 3.89, 4 runs) | 4.5 % |
| Mesh error of the model | The largest difference between n = 40 and n = 80 (`T_389`) | 0.5 % |
| Time step error of the model | The largest difference between the Courant numbers 0.5 and 0.25 (`T_389`) | 0.2 % |

`rel_tol` is the sum, 5.2 %, rounded up to 5.5 %.
The front of the model is slower than the mean at all 3 points, by 2.3 % to 3.1 %. Each difference is less than 1 standard deviation of the runs.

| Check | Model | Measured | Difference | Tolerance | Part of the tolerance |
|---|---:|---:|---:|---:|---:|
| `T_211` | 1.8717 | 1.83 | 0.0417 | 0.101 | 41 % |
| `T_300` | 2.5856 | 2.51 | 0.0756 | 0.138 | 55 % |
| `T_389` | 3.2057 | 3.11 | 0.0957 | 0.171 | 56 % |

The tolerance stops this error:

- A column of height a in place of 2a (a wrong box in `system/setFieldsDict`): T is 1.924, 2.730 and 3.482. `T_300` (8.8 % too large) and `T_389` (12 %) fail.

The tolerance does not stop a coarse mesh: n = 10 passes, because the time of the front changes little with the mesh.

## Not in the check

- The height of the column at x = 0 (Table 6 of the source). The table of the function object `front` has it: the largest y of the water.
- Z larger than 4.11: only 3 of the 9 runs of a = 2.25 in have data there, and the run takes longer.
- The other column of the source, a = 1.125 in: it has the same n^2 and thus the same T(Z) in theory.

On a Mac with 10 cores and other jobs, `fh validate` of the case took 85 s (34 s of CPU time) on 1 core with OpenFOAM v2512.
