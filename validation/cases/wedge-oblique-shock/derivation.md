# wedge-oblique-shock derivation

This case checks the compressible flow path: a steady supersonic flow over a wedge, with an attached oblique shock.
The reference is the exact solution of the oblique shock relations. Thus the tolerance comes from the mesh study and the iteration error of the model.

## Sources

- NPARC Alliance Verification and Validation Archive, NASA Glenn Research Center, "Supersonic Wedge" (J. W. Slater), https://www.grc.nasa.gov/WWW/wind/valid/wedge/wedge.html.
  Inviscid flow at Mach 2.5 over a wedge with a half-angle of 15 degrees. The leading edge is at x = y = 0. The domain has the inflow at x = -0.5, the outflow at x = 1 and the far field at y = 1. Before the wedge, y = 0 is a symmetry plane.
- The check values come from Table 2 of that page, "the analytic inviscid solution", from the program `oblshk.f` with a tolerance of 1e-6.
- Study #2 of the same archive (https://www.grc.nasa.gov/WWW/wind/valid/wedge/wedge02/wedge02.html) is a mesh study of the WIND code on 4 grids with a spacing of 0.02, 0.01, 0.005 and 0.0031. Its error of the mean Mach number behind the shock is 0.39 %, 0.16 %, 0.066 % and 0.037 %. The case uses it only as a cross-check.
- The oblique shock relations are in J. D. Anderson, Modern Compressible Flow, McGraw-Hill, and in Ames Research Staff, "Equations, tables, and charts for compressible flow", NACA Report 1135 (1953). The shock angle beta comes from the theta-beta-M relation

  tan(theta) = 2 cot(beta) (M1^2 sin^2(beta) - 1) / (M1^2 (gamma + cos(2 beta)) + 2),

  with the weak solution. With Mn1 = M1 sin(beta), the normal shock relations give the ratios p2/p1 = 1 + 2 gamma/(gamma + 1) (Mn1^2 - 1) and rho2/rho1 = (gamma + 1) Mn1^2 / ((gamma - 1) Mn1^2 + 2), and T2/T1 = (p2/p1)/(rho2/rho1). M2 = Mn2 / sin(beta - theta), with Mn2^2 = (1 + (gamma - 1)/2 Mn1^2) / (gamma Mn1^2 - (gamma - 1)/2).
  A bisection of the theta-beta-M relation in double precision gives the same 5 values as Table 2 to the last digit of the table: beta = 36.944900 degrees, M2 = 1.8735260, p2/p1 = 2.4675001, rho2/rho1 = 1.8665486 and T2/T1 = 1.3219587.

| Quantity | Table 2 | Check | Value of the check |
|---|---:|---|---:|
| Shock angle beta (degrees) | 36.94490 | `shock_angle` | 36.94490 |
| Mach number behind the shock, M2 | 1.873526 | `mach2` | 1.873526 |
| p2/p1 | 2.467500 | `pressure_ratio` | 2.467500 |
| T2/T1 | 1.321958 | `temperature_ratio` | 1.321958 |
| rho2/rho1 | 1.866549 | `density2` | rho2 = 1.4 x 1.866549 = 2.613168 |

The free stream of the model has p1 = 1, T1 = 1 and rho1 = 1.4. Thus p2, T2 and rho2/1.4 are the ratios.

## Model

The OpenFOAM case is `mach2.5`. The model has no units, and its lengths are the lengths of the NPARC case.

| Item | Value |
|---|---|
| Domain | -0.5 <= x <= 1 and y <= 1. The wedge surface is y = x tan(15 degrees) for x > 0. 1 cell of depth 0.01 in z, with an `empty` patch on the front and the back |
| Mesh | 2 blocks of `blockMesh`. Before the wedge, square cells of h = 0.01. Above the wedge, columns of width h with 100 cells each, thus the cells there are h wide and h (1 - x tan(15 degrees)) high. 15000 cells: the grid B of the NPARC study |
| Gas | perfect gas, gamma = 1.4, mu = 0. R = 1/1.4 and Cp = gamma R/(gamma - 1) = 2.5. Thus p = 1 and T = 1 give rho = 1.4 and a speed of sound of 1. The mol weight is 1.4 times the gas constant of OpenFOAM v2512, 8314.47006650545 |
| Free stream | p = 1, T = 1, U = (2.5, 0, 0): Mach 2.5 |
| Boundaries | `inlet` (x = -0.5) and `top` (y = 1): the free stream, `fixedValue`. `outlet` (x = 1): `zeroGradient`, because the flow there is supersonic. `wedge`: `slip` for U and `zeroGradient` for p and T. `symmetry` (y = 0, x < 0): `symmetryPlane` |
| Solver | `rhoCentralFoam`, the central scheme of Kurganov and Tadmor (`fluxScheme Kurganov`) with the van Leer limiter for rho, U and T |
| Time | the free stream everywhere at t = 0, then to t = 2 with the Courant number 0.2. The free stream goes through the domain in 0.6 |

The shock leaves the domain through the outlet at y = 0.752, below the far field. Thus the far field sees only the free stream.

The function objects of `system/controlDict` write the 5 values at each time step. The checks read the last row, at t = 2.

- Shock angle: the area A between the wedge and the shock for 0 < x < 1 is (tan(beta) - tan(15 degrees))/2. The `exprField` `shockAngle` gets A as the integral of (p - 1)/(2.4675 - 1) over the cells with 0 < x: the area of a sharp shock with the same pressure rise. Then it gives beta = atan(tan(15 degrees) + 2 A). A mask of the cells with p above the middle of the jump changes in steps of 1 cell. At h = 0.01, beta from such a mask changed by up to 0.08 % in the steady flow. The integral of the pressure rise changed by 0.002 %.
- The state behind the shock: the volume average of `Ma`, `p`, `T` and `rho` over the cells with 0.25 < x and y < x/2. The shock is at y = 0.752 x, thus the region stays at least 0.0715 from it, 7 cells at h = 0.01. This is the mean behind the shock of the NPARC study, without the cells in the shock.

`exprField` of OpenFOAM v2512 cannot sum a field with units: `sum(vol() * ...)` stops with a dimension error. Thus the shock angle uses `weightAverage`, the volume average, and divides by the volume average of the mask 0 < x.

## Courant number

With `maxCo 0.5`, the case ran with no warning, but the flow behind the shock was not uniform and not steady. On the line x = 0.75, p was 2.34 to 2.63, up to 7 % from the theory.
With `maxCo 0.2`, p on the same line is 2.463 to 2.474 up to 2 cells from the shock, and the flow is steady.

## Mesh study

The scratch commands below, in the repository root, solve the case on a mesh with N cells from y = 0 to y = 1 (N = 100 is the check):

```
cp -R validation/cases/wedge-oblique-shock/mach2.5 /tmp/wedge-N
# In /tmp/wedge-N/system/blockMeshDict, change (50 100 1) to (N/2 N 1), (100 100 1) to (N N 1)
# and the depth 0.01 to 1/N.
openfoam blockMesh -case /tmp/wedge-N
openfoam rhoCentralFoam -case /tmp/wedge-N
```

The values are in the last rows of the tables in `/tmp/wedge-N/postProcessing`, at t = 2. The number in brackets is the difference from the theory.

| h (NPARC grid) | Cells | Time steps | CPU time | beta | M2 | p2 | T2 | rho2 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0.02 (A) | 3750 | 1704 | 8 s | 36.98943 (+0.121 %) | 1.868786 (-0.253 %) | 2.466926 (-0.023 %) | 1.325568 (+0.273 %) | 2.605451 (-0.295 %) |
| 0.01 (B), the check | 15000 | 3404 | 51 s | 36.97097 (+0.071 %) | 1.869793 (-0.199 %) | 2.467813 (+0.013 %) | 1.324584 (+0.199 %) | 2.608328 (-0.185 %) |
| 0.005 (C) | 60000 | 6811 | 400 s | 36.95912 (+0.038 %) | 1.871578 (-0.104 %) | 2.467524 (+0.001 %) | 1.323274 (+0.100 %) | 2.610602 (-0.098 %) |
| Extrapolated from B and C, order 1 | | | | 36.94727 (+0.006 %) | 1.873363 (-0.009 %) | 2.467235 (-0.011 %) | 1.321964 (+0.000 %) | 2.612876 (-0.011 %) |
| Theory (Table 2) | | | | 36.94490 | 1.873526 | 2.467500 | 1.321958 | 2.613168 |

From B to C, the differences of beta, M2, T2 and rho2 from the theory become 1.8 to 2.0 times smaller: the observed order is 0.87 to 1.00. First order is the order of a captured shock and of the error at the tip of the wedge, see below. From A to B, M2 and T2 change less: grid A is not yet in the range where the error falls as h. p2 converges faster.
The extrapolation uses the order 1 and no reference: f = f_C - (f_B - f_C). The extrapolated values agree with the theory to 0.011 % or less.
The mean Mach number of the WIND code of the NPARC study is 0.39 %, 0.16 % and 0.066 % above the theory on the grids A, B and C. The model is 0.25 %, 0.20 % and 0.10 % below it: errors of the same size.

The error comes from the tip of the wedge. There the shock and the wall meet in 1 cell, and the scheme makes too much entropy. The flow carries this entropy along the wall, and the numerical diffusion spreads it. On the line x = 0.75 at h = 0.01, the cells up to 0.12 from the wall have T = 1.3216 to 1.3277, up to 0.4 % above the theory, with p within 0.05 % of the theory. Thus rho and Ma there are low. Farther from the wall, T and Ma are within 0.05 % of the theory.

The iteration error at t = 2 is 0.002 % or less: on grid B, the values at t = 1.5 and t = 2 differ by 0.002 % or less.

On a Mac with 10 cores, `fh validate` took 66 s for this case, while other programs used most of the cores.

## Tolerance

The tolerance is `rel_tol = 0.0025` and `abs_tol = 0`.

The reference is exact: `oblshk.f` solves the relations to 1e-6, and the bisection above gives the same values. Thus the tolerance is the mesh error of grid B, the iteration error and the error of the reference.
The mesh error of grid B is the Richardson estimate from B and C with the order 1, 2 |f_B - f_C|, times 1.25, the factor of safety of the grid convergence index.

| Cause | beta | M2 | p2 | T2 | rho2 |
|---|---:|---:|---:|---:|---:|
| Mesh error of grid B, 2 \|f_B - f_C\| | 0.064 % | 0.191 % | 0.023 % | 0.198 % | 0.174 % |
| The same, times 1.25 | 0.080 % | 0.238 % | 0.029 % | 0.248 % | 0.218 % |
| Iterations | 0.0003 % | 0.002 % | 0.002 % | 0.0002 % | 0.002 % |
| Reference | 0.0001 % | 0.0001 % | 0.0001 % | 0.0001 % | 0.0001 % |

The largest sum is 0.248 % for `temperature_ratio`. `rel_tol` is this sum, rounded up to 0.25 %.
The model is 0.013 % to 0.199 % from the theory. The largest part of the tolerance that a check uses is 80 % (`mach2` and `temperature_ratio`).
The tolerance does not let grid A pass: there `temperature_ratio` is 0.273 % and `density2` 0.295 % from the theory.
