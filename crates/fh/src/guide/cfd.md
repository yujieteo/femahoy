# CFD with OpenFOAM

For a flow request, write an OpenFOAM case folder and solve it with `fh run CASE`.

1. Case: write `system/controlDict`, `system/fvSchemes`, `system/fvSolution`, `system/blockMeshDict`, the files of `constant` and the start fields in `0`. Name the solver in `controlDict`, for example `application simpleFoam;`. A 2D case has 1 cell in z, and a patch of type `empty` on the front and the back.
2. Forces: add the function object `forceCoeffs` (type `forceCoeffs`) to `controlDict`, with `patches`, `liftDir`, `dragDir`, `magUInf`, `lRef`, `Aref` and `CofR`.
3. Solve: `fh run CASE`. `fh run` starts `blockMesh` when the case has no mesh, then `setFields` when the case has `system/setFieldsDict`, then the solver, then `foamToVTK`. A failure shows the error of OpenFOAM and the path of its log.
4. Read the numbers: `fh run` prints the folder of the logs (`log.<program>`) and of `postProcessing`. The last row of `postProcessing/forceCoeffs/0/coefficient.dat` holds the coefficients of the last iteration. The last line that starts with `#` names the columns. The page shows the cell fields of each time that the case writes (`writeInterval`), for example `U` and `p`.

This setup agrees with the measured data of the NACA 0012 airfoil at a Reynolds number of 6 million (the validation case `naca0012-ladson`): `simpleFoam` with the `SpalartAllmaras` model, `nuTilda` = 3 nu in the free stream and 0 on the wall, `freestreamVelocity` and `freestreamPressure` on a far field 500 chords away, a first cell with y+ below 1, and 1500 iterations with relaxation factors 0.9 and `consistent yes`.
It also agrees with the measured lift and pitching moment of the NACA 23012 with a plain flap of 20 % chord (the validation case `naca23012-flap`). There the mesh turns the surface aft of the hinge about the hinge, with a smooth blend over 5 % of the chord, and the wake cut goes straight downstream from the trailing edge. `CmPitch` is about `CofR`, positive nose up.

The setup below agrees with the de Vahl Davis bench mark for natural convection in a square cavity (the validation case `cavity-devahldavis`).
It is `buoyantBoussinesqSimpleFoam` with `simulationType laminar` on a uniform mesh of 129 x 129 cells. It uses `bounded Gauss linear` for `div(phi,U)` and `div(phi,T)`, the relaxation factors 0.2 for `p_rgh`, 0.8 for `U` and 0.95 for `T`, and 2000 iterations.
To get the Nusselt number, add the function object `grad` of `T`, then a `surfaceFieldValue` with `operation areaNormalAverage` of `grad(T)` on the wall patch. For the largest velocity on a line, add the function object `components` of `U`, then a `surfaceFieldValue` with `operation max` of `Ux` or `Uy` on a `sampledSurface` of type `cuttingPlane`. A mesh with an odd number of cells puts a row of cell centres on each centre line.

For a free surface of water and air, for example a dam break or a tank that sloshes, use `interFoam` (volume of fluid).
Write `phases (water air);` and the 2 fluids in `constant/transportProperties`, `constant/g`, and `alpha.water`, `p_rgh` and `U` in `0`. Put the water in `alpha.water` with `boxToCell` in `system/setFieldsDict`.
On an open top, use `totalPressure` for `p_rgh`, `pressureInletOutletVelocity` for `U` and `inletOutlet` for `alpha.water`. On a wall, use `fixedFluxPressure`, `noSlip` and `zeroGradient`.
This setup agrees with the measured surge front of the Martin and Moyce dam break (the validation case `dam-break-vof`): square cells, MULES with `cAlpha 1` and `MULESCorr yes`, and `adjustTimeStep yes` with `maxCo` and `maxAlphaCo` 0.5.
To get the front or the height of the water, add the function object `fieldExtents` of `alpha.water` with `threshold 0.5`. It writes the extents of the cell centres of the water after each time step.

For a supersonic flow with shocks, use `rhoCentralFoam` with `fluxScheme Kurganov` and `vanLeer` (`vanLeerV` for `U`) for the `reconstruct` schemes. This setup agrees with the oblique shock relations for the flow at Mach 2.5 over a wedge of 15 degrees (the validation case `wedge-oblique-shock`):

- `hePsiThermo` with `mu 0` for an inviscid flow. Then `fvSolution` needs only `"(rho|rhoU|rhoE)" { solver diagonal; }`.
- `adjustTimeStep yes` with `maxCo 0.2`. With `maxCo 0.5`, the pressure behind the shock had oscillations of up to 7 %, with no warning.
- A supersonic inflow and far field: `fixedValue` for `U`, `p` and `T`. A supersonic outflow: `zeroGradient`. An inviscid wall: `slip` for `U`.
- Solve in time from the free stream to the steady state, 3 or more times the time that the flow takes to go through the domain.
- The function object `MachNo` writes the field `Ma`. Give it `writeControl writeTime`, else it writes a time folder at each time step.
- For a mean over a region, add an `exprField` of a mask, for example `pos().x() > 0.25 ? 1 : 0`, then a `volFieldValue` with `operation weightedVolAverage` and `weightField` of the mask. In an `exprField`, `sum(vol()*...)` stops with a dimension error. Use `weightAverage(...)`.
- The tip of a wedge makes a numerical error in T and Ma near the wall. It falls as the cell size: with cells of 1/100 of the wedge length, the mean T and Ma behind the shock were 0.2 % from the theory, and with 1/200, 0.1 %.
