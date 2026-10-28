# cavity-devahldavis derivation

## Conclusion

The model agrees with the bench mark solution of de Vahl Davis (1983) at Ra = 1e4 and Ra = 1e5. The 6 checked values are 0.01 % to 0.26 % above Table V, and the tolerance is 0.6 %. The largest difference is the average Nusselt number at Ra = 1e5: 4.5309 against 4.519. The mesh error of the 129 x 129 mesh is +0.205 % for this value (see "Mesh study"), thus the mesh causes most of the difference.

Each velocity maximum of the model is at the position of Table V, within 0.003. The cell size is 0.0078.

**The horizontal velocity on the vertical centre line x = 0.5**

![The horizontal velocity on the vertical centre line x = 0.5](cavity-devahldavis/u.svg)

Table V maximum: de Vahl Davis (1983), Table V

The model has no units: L, Th - Tc and alpha are 1. The velocity is in units of alpha / L.

**The vertical velocity on the horizontal centre line y = 0.5**

![The vertical velocity on the horizontal centre line y = 0.5](cavity-devahldavis/v.svg)

Table V maximum: de Vahl Davis (1983), Table V

The model has no units: L, Th - Tc and alpha are 1. The velocity is in units of alpha / L.

The hot wall heats the air, and the air turns clockwise in the cavity. At Ra = 1e5 the buoyancy is 10 times larger. The average Nusselt number is 2.0 times larger. The largest vertical velocity is 3.5 times larger and nearer to the hot wall. Select a Rayleigh number above the view. The field list has the temperature `T` for the Nusselt number, and `U1` and `U2` for the 2 velocity checks. The dots mark the largest u on x = 0.5 and the largest v on y = 0.5.

## Result

**Pass.** A check passes when |computed - reference| ≤ rel_tol · |reference| + abs_tol, with rel_tol = 0.006 and abs_tol = 0 (`check.toml`). `fh validate` gives the computed values.

| Check | Unit | Computed | Reference | Difference | Tolerance | Margin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `nu_ra1e4` | Not specified | 2.24604 | 2.243 | +0.00303765 (+0.135428 %) | 0.01346 | 0.01042 |
| `umax_ra1e4` | Not specified | 16.1802 | 16.178 | +0.00218196 (+0.0134872 %) | 0.09707 | 0.09489 |
| `vmax_ra1e4` | Not specified | 19.6276 | 19.617 | +0.0106059 (+0.0540648 %) | 0.1177 | 0.1071 |
| `nu_ra1e5` | Not specified | 4.53088 | 4.519 | +0.0118762 (+0.262807 %) | 0.02711 | 0.01524 |
| `umax_ra1e5` | Not specified | 34.7501 | 34.73 | +0.0200793 (+0.0578153 %) | 0.2084 | 0.1883 |
| `vmax_ra1e5` | Not specified | 68.6618 | 68.59 | +0.0717973 (+0.104676 %) | 0.4115 | 0.3397 |

Made by `scripts/validation-reports.py` on 2026-10-28, femahoy 5ac0a46.

This case checks heat transfer and buoyancy in the CFD path: laminar natural convection of air in a square cavity, at 2 Rayleigh numbers.
The reference is a numerical bench mark with a known accuracy. Thus the tolerance comes from that accuracy and from the mesh study of the model, not from the difference between the model and the reference.

## Sources

- G. de Vahl Davis, "Natural convection of air in a square cavity: a bench mark numerical solution", International Journal for Numerical Methods in Fluids 3 (1983) 249-264, https://doi.org/10.1002/fld.1650030305.
  The left wall is hot, the right wall is cold, and the top and the bottom are adiabatic. Pr = 0.71.
- The check values come from Table V, the bench mark solution. The velocities are in units of alpha/L, with alpha the thermal diffusivity.
- Table VII gives the estimated error of the bench mark: 0.2 % at Ra = 1e4 and 0.3 % at Ra = 1e5.
- Table V gives 2 average Nusselt numbers for the hot wall: Nu_0 from a one-sided difference at the wall, and Nu-bar, the average over the cavity. In a steady state, the same heat goes through each vertical line. The paper tells that Nu_0 is the less accurate value. The finite volume method of the model conserves heat, thus its heat flow through the hot wall is the same as its Nu-bar. Thus the check uses Nu-bar.
- M. Hortmann, M. Peric and G. Scheuerer, "Finite volume multigrid prediction of laminar natural convection: bench-mark solutions", International Journal for Numerical Methods in Fluids 11 (1990) 189-207, https://doi.org/10.1002/fld.1650110206. It gives Nu-bar = 2.24475 at Ra = 1e4 and 4.52164 at Ra = 1e5, from meshes of up to 640 x 640 control volumes. Table 1 of T. Gjesdal, C. E. Wasberg and O. Andreassen, "Spectral element simulations of buoyancy-driven flow", https://arxiv.org/abs/physics/0305049, also gives these values. The case uses them only as a cross-check.

| Ra | Nu-bar | u_max on x = 0.5 (at y) | v_max on y = 0.5 (at x) | Error of the bench mark | Checks |
|---:|---:|---|---|---:|---|
| 1e4 | 2.243 | 16.178 (0.823) | 19.617 (0.119) | 0.2 % | `nu_ra1e4`, `umax_ra1e4`, `vmax_ra1e4` |
| 1e5 | 4.519 | 34.73 (0.855) | 68.59 (0.066) | 0.3 % | `nu_ra1e5`, `umax_ra1e5`, `vmax_ra1e5` |

The paper uses z for the vertical axis and w for the vertical velocity. The model uses y and v.

## Model

The 2 OpenFOAM cases `ra1e4` and `ra1e5` differ only in `constant/g`.
The model has no units. The side L, the temperature difference Th - Tc and the thermal diffusivity alpha are 1. Thus the velocities of the model are in units of alpha/L, as in Table V.

| Item | Value |
|---|---|
| Cavity | the square 0 <= x, y <= 1, with 1 cell of depth 0.1 in z and an `empty` patch on the front and the back |
| Walls | no slip on each wall. `hot` (x = 0): T = 1. `cold` (x = 1): T = 0. `adiabatic` (y = 0 and y = 1): zero gradient of T |
| Fluid | nu = 0.71 and Pr = 0.71, thus alpha = nu / Pr = 1. beta = 1 and TRef = 0.5 |
| Gravity | Ra = g beta (Th - Tc) L^3 / (nu alpha), thus g = Ra Pr in -y: 7100 for `ra1e4` and 71000 for `ra1e5` |
| Solver | `buoyantBoussinesqSimpleFoam`, laminar and steady |
| Schemes | `bounded Gauss linear` for the convection of U and T, `Gauss linear` for the gradients, `Gauss linear corrected` for the Laplacians |
| Relaxation | 0.2 for `p_rgh`, 0.8 for `U` and 0.95 for `T` |
| Mesh | uniform, 129 x 129 cells |
| Iterations | 2000 |

The function objects of `system/controlDict` write the 3 values of each case:

- Nusselt number: the `areaNormalAverage` of `grad(T)` on the patch `hot`. The normal points out of the fluid, thus the value is the average of -dT/dx on the hot wall.
- Velocities: the largest `Ux` in the row of cells that the plane x = 0.5 cuts, and the largest `Uy` in the row of cells that the plane y = 0.5 cuts. The mesh has an odd number of cells, thus each plane goes through a row of cell centres.

With the relaxation factors 0.7, 0.3 and 0.5, the Nusselt number at Ra = 1e4 on 129 x 129 changed between 1.5 and 2.34 for 8000 iterations. The relaxation factors of the case get a residual of 1e-7 after about 2000 iterations.

## Mesh study

These commands, in the repository root, solve the case `ra1e5` on a mesh of 257 x 257 cells in a scratch folder:

```
cp -R validation/cases/cavity-devahldavis/ra1e5 /tmp/cavity-257
# In /tmp/cavity-257/system/blockMeshDict, change (129 129 1) to (257 257 1).
# In /tmp/cavity-257/system/controlDict, change endTime and writeInterval to 10000.
openfoam blockMesh -case /tmp/cavity-257
openfoam buoyantBoussinesqSimpleFoam -case /tmp/cavity-257
```

The values are in the last rows of the tables in `/tmp/cavity-257/postProcessing`.
At 257 x 257, the study stopped Ra = 1e4 at 10000 iterations and Ra = 1e5 at 7520 iterations. There the residual of each equation was below 1e-10.

| Mesh | Iterations | Nu, 1e4 | u_max, 1e4 | v_max, 1e4 | Nu, 1e5 | u_max, 1e5 | v_max, 1e5 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 33 x 33 | 2000 | 2.26332 | 16.1298 | 19.5352 | 4.65992 | 34.8516 | 67.3526 |
| 65 x 65 | 2000 | 2.24962 | 16.1782 | 19.6247 | 4.55791 | 34.7948 | 68.5601 |
| 129 x 129 | 3000 | 2.24604 | 16.1801 | 19.6276 | 4.53088 | 34.7501 | 68.6618 |
| 257 x 257 | 10000, 7520 | 2.24512 | 16.1830 | 19.6285 | 4.52397 | 34.7439 | 68.6147 |
| Extrapolated | | 2.24481 | | | 4.52159 | | |
| 129 x 129, the check | 2000 | 2.24604 | 16.1802 | 19.6276 | 4.53088 | 34.7501 | 68.6618 |

The Nusselt number converges monotonically. The extrapolation from the meshes 65, 129 and 257 uses the observed order p, which is 1.97 at each Rayleigh number. The extrapolated values agree with Hortmann et al. to 0.003 % or less.
Thus the mesh error of 129 x 129 is +0.055 % in Nu at Ra = 1e4 and +0.205 % at Ra = 1e5.

A velocity check reads the largest cell value, not the maximum between the cell centres. Thus a velocity changes less smoothly with the mesh. A `sets` function object on the centre lines and a parabola through the 3 largest cell values give the maximum between the cell centres. At 129 x 129, the largest cell value is 0 % to 0.016 % below this maximum.
The velocities at 129 x 129 are within 0.03 % of the maxima of the parabolas at 257 x 257.

The iteration error at 2000 iterations is 0.0003 % or less: on 129 x 129, the values at 2000 and 3000 iterations differ by 0.0003 % or less.

The 2 cases of the check run at the same time. On a Mac with 10 cores, `fh validate` took 47 s and 88 s for this case, while other programs used 3 to 5 cores.

## Tolerance

The tolerance is `rel_tol = 0.006` and `abs_tol = 0`.

| Cause | Ra = 1e4 | Ra = 1e5 |
|---|---:|---:|
| Error of the bench mark (Table VII) | 0.2 % | 0.3 % |
| Mesh error of 129 x 129 in Nu, times 1.25 (the factor of safety of the grid convergence index) | 0.07 % | 0.26 % |
| Mesh error of 129 x 129 in the velocities, times 1.25 | 0.04 % | 0.04 % |
| The largest cell value against the maximum between the cell centres | 0.02 % | 0.02 % |
| Iterations | 0.0003 % | 0.0001 % |

The sum for a Nusselt number is 0.27 % at Ra = 1e4 and 0.56 % at Ra = 1e5. The sum for a velocity is 0.26 % and 0.36 %.
`rel_tol` is the largest sum, 0.56 % for `nu_ra1e5`, rounded up to 0.6 %.
The model is 0.01 % to 0.27 % above the bench mark. The largest part of the tolerance that a check uses is 44 % (`nu_ra1e5`).
The tolerance does not let a mesh of 65 x 65 pass: there `nu_ra1e5` is 0.86 % above the bench mark.
