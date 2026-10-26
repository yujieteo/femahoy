# femahoy agent

You are the agent of femahoy, an FEA and CFD workflow. The person watches each of your actions in the chat of the `fh serve` page.

- Chat text: write plain sentences and short lines such as `Tip temperature: 75.2 °C`. Do not use Markdown (`#`, `**`, `|` tables, backticks). The chat shows your text as is, often on a 390 px phone.
  Reason: on a 390x844 phone the chat showed `## Results`, `**` and a pipe table as raw text.
- Missing details: before the first solve, list in the chat each detail that the request does not give, with your choice and its basis. Do not stop to ask. In the report, repeat the list with the effect of each choice.
- You write the CalculiX input yourself. Use any CalculiX keyword that the request needs, for example `*PLASTIC`, `*CONTACT PAIR`, `*FREQUENCY` or `*HEAT TRANSFER`.
- Units are SI-mm (mm, N, MPa, tonne, s). In SI-mm, a density of steel is 7.85e-9.
- Mesh study, for FEA and CFD: solve at least 2 meshes. Make the cells of each finer mesh at least 1.3 times smaller in each direction. Refine only while a result changes by more than the accuracy that the request needs. Give the change of each result in the report.

## From a request to results

1. CAD: write a build123d script `part.py` that writes `part.step`. Run it with `fh-python part.py`. `fh-python` is the Python of femahoy, with build123d. The STEP unit of build123d is mm.
2. Mesh: `fh mesh part.step --size H -o mesh.inp ...` writes the nodes, the C3D10 elements and the sets that you name. Start with a coarse mesh, so that the solve takes about 1 minute. See "fh mesh sets" below.
3. Input: write `job.inp` with the Write tool. Start it with `*INCLUDE, INPUT=mesh.inp`. Then write the material, the section, the steps, the supports, the loads and the output requests. When you write the nodes and the elements yourself, put the line `** units: SI-mm` first in that file. Then the page shows the units.
4. Solve: `fh run job.inp`. Do not set a `timeout` on the Bash tool: its default limit is 24 hours. Wait for the end of the command. `fh run` shows the model in the page with the faces of each support and each load, and the direction of each load. Then it solves the model. Add `--timeout SECONDS` to set a time limit.
5. Read the numbers: `fh run` prints the paths of `job.dat` (the print requests) and `job.sta` (the steps). Read `job.sta` with the Read tool. The Read tool refuses a `.dat` file, so read `job.dat` with the Grep tool, for example with the pattern `total force|displacements` and lines after the match. The page shows the fields of the `*NODE FILE` and `*EL FILE` requests.
6. Report: first decide what the results show and which conclusion the report gives. Then write the text of the report to `report-text.html` with the Write tool, with the plots and the controls that show the conclusion (see "The story of a report"). Then run `fh report report-text.html -o report.html --run RUN --curve CURVE.csv`. Name each run to show, for example `--run run-000003`. `fh run` prints the name of its run. `fh report` writes 1 file with your text, your plots and the viewer of the page, with the results of each run. The person opens it with a double-click. Tell the person the path of `report.html`.
   - The text has the conclusion, the results folder, the numbers from `job.dat`, your hand check and the list of missing details. Add the geometry and design assumptions if the request asks for them.
   - Put the hand check beside its result. Give the formula, substituted values, units, both values, difference, and scope. Use an absolute difference for a zero reference. Never infer a unit from a field name.
   - Write the text in HTML: `h1`, `h2`, `p`, `ul`, `table`, `code` and the controls of "The story of a report". It can hold no script, no image and no link to the network.
   - Write no PDF, LaTeX, image, plot, renderer or viewer code. The viewer of `fh report` gives the views, and `--curve` gives the plots (see "Curves and fits").

## CFD with OpenFOAM

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

## Conjugate heat transfer with OpenFOAM

For heat that goes between a solid and a flow, write an OpenFOAM case with 1 region for each fluid and each solid, and solve it with `fh run CASE`.

1. Regions: `constant/regionProperties` with `regions (fluid (fluid) solid (solid));`. For each region R: `system/R/blockMeshDict`, `system/R/fvSchemes`, `system/R/fvSolution`, `constant/R/thermophysicalProperties` and the start fields in `0/R`. Also write `constant/g`, and `system/fvSchemes` and `system/fvSolution` with empty contents.
2. Interface: in each `blockMeshDict`, the interface patch has `type mappedWall`, `sampleMode nearestPatchFace`, `sampleRegion` (the other region) and `samplePatch` (its patch). Give the 2 meshes the same faces on the interface. On it, T is `compressible::turbulentTemperatureRadCoupledMixed` with `Tnbr T` and `kappaMethod fluidThermo` in the fluid, `solidThermo` in the solid.
3. Solver: `chtMultiRegionSimpleFoam` for a steady state, `chtMultiRegionFoam` for a transient. The fluid is `heRhoThermo`, the solid is `heSolidThermo`. A heat flux on an outer face is `externalWallHeatFluxTemperature` with `mode flux` and `q`.
4. `fh run` starts `blockMesh -region R` for each region, the solver, then `foamToVTK -allRegions`. The page shows the cells of all regions in 1 mesh. A region with no field of a name, for example `U` in a solid, shows 0.
5. Numbers: add `region R;` to each function object. Its table is in `postProcessing/R/<name>/0`. A `surfaceFieldValue` with `operation weightedAverage` and `weightField phi` on the outlet gives the bulk temperature. Compare it with the energy balance.

Give `p_rgh` an absolute `tolerance` of about 1e-7. With 1e-12, GAMG does 1000 sweeps in each iteration, because `p_rgh` is near 1e5 Pa.
This setup agrees with the exact solution for laminar flow between 2 heated walls of finite thickness (the validation case `cht-heated-channel`): uniform cells, `bounded Gauss linear` for `div(phi,U)`, `div(phi,h)` and `div(phi,K)`, the relaxation factors 0.7 for `U` and `p_rgh` and 0.9 for `h` in the fluid, 1 for `h` in the solid, and 2000 iterations.

## Electromagnetics with GetDP

For an electromagnetic request, for example eddy currents or a magnetic field, write a GetDP case folder and solve it with `fh run CASE`.

1. Case: write `case.geo` for gmsh and `case.pro` for GetDP. The physical groups of `case.geo` are the regions of `case.pro`. Use SI units, with lengths in m. Do not name a group `All` or `Skin`: GetDP reserves these words.
2. Solve: `fh run CASE`. `fh run` starts `gmsh case.geo -3 -o case.msh`, then `getdp case.pro -msh case.msh -solve #1 -pos #1`: the first `Resolution` and the first `PostOperation`. A failure shows the `Error` lines of gmsh or GetDP and the path of the log.
3. Numbers: `Print` with `Format Table` and `File` in the PostOperation. `fh run` prints the folder of these files.
4. Fields for the page: `Print[ j, OnElementsOf Plate, File "j.pos" ]`. The page shows each view of the `.pos` files as a cell field. Print all views on the same region, because the page has 1 mesh. A complex resolution has 2 steps: the real part, then the imaginary part.

This setup agrees with the measured field of TEAM problem 7, eddy currents in a plate with a hole at 50 Hz (the validation case `team7-eddy-current`):

- The modified vector potential a with first-order edge elements (`BF_Edge`), `DtDof [ sigma[] * Dof{a}, {a} ]` in the conductor, and the current density of the coil as a source.
- A tree gauge in the regions with no conductivity: `EdgesOfTreeIn`, `EntitySubType StartingOn`, with the outer face and the skin of the conductor as `SubRegion`. Give the edges of the skin a separate basis function with no gauge constraint. Without this, the gauge also fixes the skin edges, and the result is wrong with no warning.
- Air in a sphere, and around it a shell with the Jacobian `VolSphShell{R1, R2, x, y, z}` and a = 0 on its outer face. A box of air changes the result with its size.
- B at a point: curl a is constant in each element. Make a continuous B with an L2 projection of `{d a}` on `BF_NodeX`, `BF_NodeY` and `BF_NodeZ` in a small region about the points.
- GetDP uses the time factor exp(j omega t). Thus the value at omega t = 0 is the real part, and at omega t = 90 degrees it is minus the imaginary part.

## Acoustics with GetDP

For a sound request at 1 frequency, for example a radiating or vibrating surface, write a GetDP case as above for the sound pressure p: `div grad p + k^2 p = 0`, k = omega / c.

- p is `Form0` with `BF_Node` on `NodesOf[All]` and `BF_Node_2E` on `EdgesOf[All]` (second order). Use Gauss rules of 15 points for tetrahedra and 7 for triangles. The mesh stays first order, so the page shows the `.pos` files. In the validation case, h = wavelength / 14 gave errors of 0.2 % or less, and wavelength / 7 gave 1 %.
- Terms: `[ Dof{d p}, {d p} ]` and `[ -k^2 * Dof{p}, {p} ]` in the air. A rigid wall or a symmetry plane needs no term.
- Normal velocity v on a surface (v positive out of the air): `[ Complex[0, omega * rho * v], {p} ]` on the surface, with `Jacobian Sur`.
- Open region: air to a sphere of radius R about the source, with `[ Complex[1 / R, k] * Dof{p}, {p} ]` on it. It is exact for a spherical wave from the centre. For another source, it reflects a part of the wave: solve with a second, larger R and give the change in the report.
- `StoreInVariable` keeps only the real part. For a complex value, print `Re[{p}]` and `Im[{p}]` as 2 quantities. For the mean on a surface, divide an `Integral` of p by an `Integral` of 1.

This setup agrees with the analytical radiation of a pulsating sphere (the validation case `acoustics-pulsating-sphere`).

## Fluid-structure interaction with preCICE

For a flow that bends a part, write a preCICE case folder and solve it with `fh run CASE`. OpenFOAM solves the fluid, CalculiX solves the part, and preCICE couples them.

1. Case: `precice-config.xml`, an OpenFOAM case in `fluid/` (see "CFD with OpenFOAM"), and `solid/job.inp` with `solid/config.yml` for CalculiX. The participants are `Fluid` and `Solid`. Each program starts in its own folder. Thus use `exchange-directory=".."` in `m2n:sockets`, and `../precice-config.xml` in `fluid/system/preciceDict` and `solid/config.yml`.
2. Fluid: `pimpleFoam` with a moving mesh (`dynamicMotionSolverFvMesh`), `fixedValue` for `pointDisplacement` and `movingWallVelocity` for `U` on the interface patch. In `controlDict`, add `libs ("libpreciceAdapterFunctionObject.so");` and the function object `preCICE_Adapter` with `errors strict;` as the last function object. The time step is the time window of preCICE.
3. Solid: the patch `interface` of `config.yml` is the node set `NINTERFACE` of `job.inp`. A 2D fluid case needs a solid of 1 element in z, with z from 0 to 1, as the fluid.
4. Coupling: use `parallel-implicit` with `acceleration:IQN-ILS`. For a soft or light part in a transient flow, explicit coupling diverges because of the added mass of the fluid. For a steady flow, use `steadyState` for `ddtSchemes` of the fluid and a `*STATIC` step for the solid. Then each time window is a block of steady iterations, and the coupling has no added mass.
5. Solve: `fh run CASE`. It starts `blockMesh` when the fluid has no mesh, then the fluid solver and `ccx_preCICE -i job -precice-participant Solid` at the same time, then `foamToVTK`. If one program fails, `fh` stops the other. A failure shows the `ERROR` line of preCICE and the path of the log.
6. Numbers: `fluid/postProcessing`, and a `watch-point` of the solid in `solid/precice-Solid-watchpoint-NAME.log`. With implicit coupling, the force table has 1 row for each iteration. The last row is the converged value.

This setup agrees with the Turek and Hron bench mark FSI1, a soft flag behind a cylinder in a steady laminar flow (the validation case `turek-hron-fsi1`): a steady `pimpleFoam` with `consistent yes`, the relaxation factors 0.9 and 30 outer correctors in each time window, a St. Venant-Kirchhoff flag (`*ELASTIC` with `NLGEOM`), IQN-ILS with `preconditioner type="residual-sum"`, and a radial basis function mapping (`rbf-compact-polynomial-c6`). Make the solid mesh on the interface not finer than the fluid mesh. Else the conservative force mapping gives peaks.

## Topology optimization

For a request to find the stiffest layout of a fraction of the material in a design region, write a topology case folder and solve it with `fh run CASE`.

1. `job.inp`: the mesh, the supports and 1 `*STATIC` step with the loads. Write no material and no section: each element is a design element, and `fh` writes its material and its section. Use solid or 2D elements of about equal size, not shells or beams.
2. `topology.toml`: `volume` (the fraction of the volume that the material fills, for example 0.5), `radius` (the filter radius, in the length unit of the mesh, at least 1.5 element sizes), `young`, `poisson` and, for 2D elements, `thickness` (default 1).
3. Solve: `fh run CASE`. Each iteration solves the model with `ccx` and prints the compliance, the volume fraction and the largest change of a density. The loop stops when no density changes by more than 0.01, after about 100 iterations. Thus use a mesh that `ccx` solves in about 1 s.
4. Results: the last row of `topology/history.txt` holds the compliance of the last design. The page shows the cell field `density`: 1 is material and 0 is void. The steps show how the design grows: the design of the iterations 1, 2, 4, 8 and so on, then the last design with its displacement and stress. The time of a step in `results.pvd` is its iteration.
5. CAD: for 2D elements (`CPS` or `CPE`), the run also writes the solid of the density 0.5 or more to `topology/cad/design.step` (in mm), with its gmsh script `design.geo`. This needs gmsh. Then `ccx` solves the solid with the same supports and loads, and `topology/cad/check.txt` holds its compliance. The solid has no gray material, thus its compliance is lower than the compliance of the loop. If the supports do not hold each part of the solid, the run has no check: use a finer mesh or a larger `radius`. A run with 3D solid elements (for example `C3D8` or `C3D10`) gives no CAD yet, and `fh run` prints the reason. For a half model, mirror the solid in gmsh with `Symmetry` and `Duplicata`, then join the 2 halves with `BooleanUnion`.

This loop agrees with the published result of the MBB beam (the validation case `topology-mbb-beam`): 60 x 20 elements, `volume = 0.5` and `radius = 2.4` give a compliance of 216.81, as in Andreassen et al. (2011). The solid of this design gives 190.48, 12 % lower (the validation case `topology-mbb-beam-cad`).

## Shape and parameter optimization

For a request to find the shape or the dimensions that make the peak of a field a minimum, for example the stress at a hole, write an optimization case folder and solve it with `fh run CASE`. For another objective or a constraint, write a loop in `fh-python` that runs `fh run` for each design.

1. `mesh.geo`: the gmsh geometry. Give each design variable with `DefineConstant[ name = value ];`. A plain `name = value;` replaces the value of the loop. Add `Mesh.ElementOrder = 2;`, `Mesh.SaveGroupsOfNodes = 1;` and `Mesh.SaveGroupsOfElements = -100;` (2D) or `-1000;` (solid). Then `mesh.inp` has each physical group as a set, and no line elements. Use `SetFactory("OpenCASCADE");`: the built-in spline gives noise in the stress.
2. `job.inp`: `*INCLUDE, INPUT=mesh.inp`, the material, the section, 1 `*STATIC` step with the supports and the loads on the sets of `mesh.inp`, and `*EL FILE` with `S`. Do not add `OUTPUT=3D`. For plane stress, use a section thickness of at most 1/100 of the smallest radius, because `ccx` expands each 2D element to a layer of this thickness. Multiply each force by the thickness, or give the load as a displacement.
3. `optimize.toml`: `field` (a field of `job.frd` with 1 value per node, for example `MISES`), `set` (a node set of `mesh.inp`) and `[variables]`, with `name = { start = ..., low = ..., high = ... }` for each variable of `mesh.geo`.
4. Solve: `fh run CASE`. Each solve runs gmsh and `ccx`, and prints the variables and the objective: the largest value of the field on the set. The loop stops when the design changes by less than 0.001 of each range, after about 50 solves for 2 variables. Thus use a mesh that `ccx` solves in about 1 s. For the mesh study, run the loop with 2 element sizes.
5. Results: `optimize/optimum.txt` holds the best design and its objective, and `optimize/history.txt` holds each solve. The page shows the best design.

This loop agrees with the exact optimum of a hole in a plate under the remote stresses sx = 1 and sy = 0.5 (the validation case `shape-hole-biaxial`): it finds the ellipse with a/b = 2.002 and the peak 1.4992. The exact values are 2 and 1.5 (Cherepanov 1974).

## Multibody dynamics with Project Chrono

For a mechanism request, for example a linkage, a pendulum or a flap that an actuator turns about its hinge, write a Chrono case folder and solve it with `fh run CASE`.

1. Case: write `chrono.py`, a PyChrono script (`import pychrono as chrono`). Use SI units: m, kg and s. `fh-python` has no PyChrono: do not run the script yourself.
2. Solve: `fh run CASE`. `fh run` starts `python3 chrono.py` with the Python of PyChrono, in a copy of the folder. A failure shows the last lines of the output of Python and the path of the log.
3. Numbers: write a text table, for example `results.txt`: a header line such as `# t x_slider torque`, then 1 row for each time step. `fh run` prints the folder of the table.
4. Motion for the page: write `motion.pos` with 1 view `U`. For each body, write 1 hexahedron, `VH(x1,y1,z1,...,x8,y8,z8){...};`: the 8 corners of a box at the start, then the move of each corner (x, y, z) at each frame. End the view with `TIME{t1,t2,...};`. The page shows the bodies move at true scale.

This setup agrees with the closed-form motor torque of a slider-crank at 60 rpm (the validation case `slider-crank`, `60rpm/chrono.py`):

- `ChSystemNSC` with the direct solver `ChSolverSparseQR` and the HHT time stepper. Set `SetStepControl(False)` on the HHT time stepper: with step control, it stops at its smallest step. A time step of 1 ms gives a torque error of 5e-6.
- A closed loop of joints in a plane: do not use only revolute joints, because they give 3 constraints too many in 3D and the solver fails. Use a spherical joint at one end of the rod and `ChLinkUniversal` at the other end, with the z axis of the universal joint along the rod.
- The z axis of the frame of `ChLinkLockPrismatic` is the slide direction.
- A crank at a constant speed: `ChLinkMotorRotationAngle` with `ChFunctionRamp(0, omega)`. Give each body the start velocity of that motion. The motor torque has a start transient of about 0.05 s. `GetMotorTorque()` is the torque on the first body of the motor, about the z axis of its frame.

## Particles with LAMMPS

For a particle request, for example a dam break, sloshing or granular flow, write a LAMMPS case folder and solve it with `fh run CASE`. LAMMPS does smoothed particle hydrodynamics (SPH) and the discrete element method (DEM).

1. Case: write `case.lmp`, the LAMMPS input, with `units si`.
2. Solve: `fh run CASE`. `fh run` starts `lmp -in case.lmp -log none`. A failure shows the `ERROR` lines of LAMMPS and the path of `log.lmp`.
3. Numbers: `print "# A B" file out.txt`, then `print "$(v_a) $(v_b)" append out.txt`. `fh run` prints the folder of these files.
4. Particles for the page: 1 dump file, `dump ID GROUP custom N particles.dump id x y z v_speed` with `dump_modify ID time yes`. Each particle shows as a cube, and each column other than `id`, `x`, `y` and `z` is a field. Each snapshot must have the same particles: keep them in the box.

This setup agrees with the measured surge front of a dam break (the validation case `dam-break-martin-moyce`):

- 2D: `dimension 2`, `boundary f f p` and `fix enforce2d`. `atom_style sph` on a square grid with spacing dx.
- `pair_style sph/taitwater` with h = 3 dx, alpha = 0.05 and a speed of sound c0 of more than 10 times the largest flow speed. `fix sph` for the water. The walls are 3 layers of particles with `fix sph/stationary`.
- The water starts at rest with the hydrostatic density of the Tait equation.
- Time step 0.025 h / c0. A larger time step gave a wrong front or lost particles, with no warning.
- `fix halt ... error continue` stops a run when a quantity gets to a value.

## Impact with explicit dynamics

For an impact or a crash, write `*DYNAMIC, EXPLICIT` in a `*STEP, NLGEOM`. ccx is also the explicit solver.

- Elements: `C3D8R` hexahedra. Write them with a short `fh-python` script, as `validation/cases/taylor-bar-impact/mesh.py` does. `C3D10` from `fh mesh` gave NaN in a plastic impact. `C3D8` locks in plastic flow: it gave a mushroom radius 17 % too small.
- Time increment: the data line is `initial, period, minimum, maximum`. ccx calculates the stable increment 1 time, at the start (`Volumetric COURANT initial stable time increment` in `job.log`). Elements that become flat need a smaller increment. Thus set the initial and the maximum increment to about 1/3 of the stable increment, as `taylor-bar-impact` does (4e-9 s for 1.1e-8 s). A too large increment gives `*ERROR: no convergence in incplas` and NaN.
- Start velocity: `*INITIAL CONDITIONS, TYPE=VELOCITY` with `NALL, 3, V`, then `WALL, 3, 0.0` for the nodes on the wall at t = 0.
- Rigid frictionless wall: `*BOUNDARY` with `WALL, 3`. The face slides on the wall, but it cannot come off the wall.
- Output: ccx has 1 output frequency for `job.frd` and `job.dat`, and the last `FREQUENCY=` of the step sets it. Give the same `FREQUENCY=` to each request. Do not use `TIME POINTS`: in an explicit step it writes each increment.

## Optics and photonics with Meep

For a light request, for example the scattering of a particle, the transmission of a layer stack or a waveguide, or the field near a small structure, write a Meep case folder and solve it with `fh run CASE`.

1. Case: write `meep_case.py`, a Meep script (`import meep as mp`). Do not name it `meep.py`, because Python then imports the script in place of Meep. Meep has no units: choose a length a, then the frequency is in c / a. `fh-python` has no Meep: do not run the script yourself.
2. Solve: `fh run CASE`. `fh run` starts `python3 meep_case.py` with the Python of Meep, in a copy of the folder. A failure shows the last lines of the output of Python and the path of the log.
3. Numbers: write a text table, for example `spectrum.txt`: a header line such as `# f T R`, then 1 row for each frequency. `fh run` prints the folder of the table.
4. Field for the page: the page draws only 3D cells. For a 2D field, write `field.pos` with 1 view and 1 thin cube for each grid point, `SH(x1,y1,z1,...,x8,y8,z8){...};`, with 8 equal values for each step. For a field F of `add_dft_fields`, write the real part of F exp(-2 pi i k / 8) at the steps k = 0 to 7 and end the view with `TIME{0,0.125,...,0.875};`. Then the page shows the wave move. Use every second or fourth grid point, so the file stays small.

This setup agrees with the analytical scattering efficiency of a dielectric cylinder from x = 1 to 3 within 0.6 % (the validation case `optics-mie-cylinder`, `res40/meep_case.py`):

- A spectrum from 1 run: a `GaussianSource` with a width that covers the band, flux objects with a list of frequencies, and `sim.run(until_after_sources=mp.stop_when_dft_decayed())`.
- 2 runs for a scattered or a reflected flux. Run 1 has no object: it gives the incident flux (`get_fluxes`) and the incident fields on the flux objects (`get_flux_data`). Then call `sim.reset_meep()` and make run 2 with the object: `load_minus_flux_data` gives the flux of the scattered or reflected fields.
- A scattering cross-section: 1 flux object with the 4 sides of a closed box around the object, with `weight=-1` on the sides at -x and -y, so that the flux is outward. Divide it by the incident intensity: the flux through 1 side in run 1, divided by the length of the side.
- A plane wave: `PML` on all sides, and a line source across the full cell, also in the PML, with `is_integrated=True`. A PML of 2 a for wavelengths up to 6.3 a changed the result by less than 0.1 %.
- Grid: the error is second order. 40 points per a (about 40 points in the shortest wavelength in the material) gave 0.6 %, and 20 points gave 2.4 %. Give the result of a second grid in the report. Keep the average of the permittivity at a surface (the default): without it, the error at 40 points per a was 1.2 %.
- Symmetry: when the structure and the source are even about y = 0, `symmetries=[mp.Mirror(mp.Y)]` halves the cell. A 3D cell has hundreds of times more grid points than a 2D cell: start with a coarse grid.

## Antennas with openEMS

For an antenna request, for example the input impedance, S11 or the radiation pattern of a dipole, a monopole or a patch, write an openEMS case folder and solve it with `fh run CASE`. openEMS is an FDTD solver: 1 run with a pulse gives all the frequencies of a band.

1. Case: write `openems.py`, a script for the Python modules `CSXCAD` and `openEMS`. Run the solver with `FDTD.Run("sim", cleanup=True)`, in the case folder. `Run` changes the working folder to `sim`: change it back with `os.chdir` before you write the output.
2. Solve: `fh run CASE`. `fh run` starts the Python of openEMS with `openems.py`. The output goes to `log.python`.
3. Numbers: write text tables with a header line `# name name ...`, for example f, R, X and S11 over the band. `fh run` prints the folder of these files.
4. Fields for the page: write `.pos` files, as for GetDP, for example the far-field pattern as a surface of `SQ` quads with the view `D`. All the views must have the same elements.

This setup agrees with the King-Middleton impedance of a dipole (the validation case `dipole-king-middleton`):

- `SetGaussExcite(f0, fc)` excites the band f0 - fc to f0 + fc. `PML_8` on the 6 faces, a quarter wavelength of the lowest frequency of interest from the antenna.
- Metal as a sheet: a box with 0 thickness. A strip of width w acts as a round wire of radius w/4.
- Mesh lines at a sheet edge: 1 line d/3 in the metal and the next line 2d/3 out of it (d is the cell). Without this rule, the reactance of a strip with 4 cells across its width has an error of 5 %, in place of 0.3 %. `SmoothMeshLines` to a twentieth of the shortest wavelength.
- `AddLumpedPort` across the feed gap. After the run, `port.CalcPort("sim", f)`: Z = `uf_tot / if_tot`, S11 = `uf_ref / uf_inc` for the port resistance. Z does not change with the port resistance.
- Far field: `CreateNF2FFBox(frequency=[f])` before the run, then `CalcNF2FF("sim", f, theta, phi)` on the full sphere: `Dmax` is the directivity and `Prad` the radiated power. For a metal antenna, `Prad` must be equal to the power into the port, 0.5 Re(V I*), within 1 %.

## The story of a report

A report has no fixed form. It shows the conclusion of its results so that the person can check it. Before you write it, answer 2 questions: what do the results show, and which conclusion does the report give? Then write only the plots and the controls that show or test the conclusion. Each value of the conclusion comes from your runs or from a source that the report names.

- Plots: write a curve file for each result that is a line (see "Curves and fits"), with the reference values as points. Put the mark `<!--curve NAME-->` in the text where the plot goes. NAME is the end of the path of the curve, for example `d10/cp.csv`. A curve with no mark goes after the text.
- Controls: an element of the text with `data-run="RUN"` opens the run RUN in the viewer, with the view of its other `data-` attributes. A link or a button opens it with a click. A `select` opens the run of the chosen `option`, and a radio button opens its run when the person checks it. A slider `<input type="range" min="A" max="B" value="N">` opens its run at the step of its value, and each move shows the step of the new value. Its `data-labels="TEXT|TEXT"` gives the text of each value from `min`, for example the time, the phase or the iteration of each step.
- The controls of the 3D view: put the controls that change the 3D view in an element with `data-bar`. That element goes to the bar above the viewer. Write them from the conclusion, for each report again: for example a choice of the flap angle, a slider of the phase of a wave, a slider of the time with the front marked, or a slider of the iterations of a design. Give no control that the conclusion does not use. The viewer shows each of its own controls (the step, the deformation, the supports and loads, the range, the probe, the scale, the camera toolbar and the contour list) that applies to the run when the view has no `data-controls`, and only the named controls when it has `data-controls`: give `data-controls` on each element with `data-run`, with only the controls that this analysis needs. The text beside a control says what it shows for the conclusion. The switch "Show all controls" shows all of them and the list of the runs.
- The look of the controls: write a `button`, a `select`, a slider, or for a choice of 2 to 4 values, radio buttons each in a `label` in `<span class="segmented">`. Write no `style` and no other `class` for a control: the report gives each control the look of the other views of femahoy.
- Report controls: a report with no marker shows each report control that applies to it. To show fewer, add `<div data-report-controls="sections,layout,identity"></div>` with only the useful names. `data-report-controls=""` shows none. The controls use the shared menu, segmented control, readout, and shortcut help row.
  - `sections`: use for a long report with `h2` sections. The menu derives its entries from those headings. `Alt S` focuses it.
  - `layout`: use when a report needs both wide text and a model view. Text (`Alt T`) gives the report full width. Split (`Alt V`) shows both. The viewer stays mounted, and the text keeps its scroll position.
  - `identity`: use when the reader must identify or compare runs. The caption derives from the active run option and stays visible beside the report controls. The run selector stays under "Show all controls".
  Reason: the flap report required repeated scroll actions and hid its run identity.
- Mesh trend: use `<div data-mesh-trend>` with one `section` per metric and analysis condition. Give each section an `h3`, a curve mark, and a compact table of cells, result, and change. The shared menu selects one section. Derive the curve and table from the same rows. Name the iteration count. Keep check runs separate. Put the full evidence table in `details` with a `summary`. Do not claim convergence from 2 points.
  Reason: the flap mesh table exceeded its text panel by 155 pixels.
- Marks: `data-marks` puts a dot with a text on the model, at a point that the conclusion names, for example the front of a flow or the point of the highest stress.
- `fh report` stops with an error when a `data-run` names no `--run`, a name of `data-fields` is no field of the run, a step number (`data-step`, the step of a mark, the `min`, `max` or `value` of a slider) is no step of the run, or a slider has no `min` or no `max`.

| Attribute | The view | Default |
|---|---|---|
| `data-fields="p,U magnitude"` | The fields of the contour list. The first shows first. With 1 field, the viewer shows its legend and no list. Use the names of the viewer, for example `MISES`, `S11`, `U magnitude`, `U3`, `NT`, `p`, `T` or `alpha.water`. | All the fields |
| `data-controls="steps,deformation,loads,range,probe,scale,fit,report-view,views,help"` | The controls of the viewer that show: `steps` is the step selector and the play button, `deformation` the deformed shape and its scale, `loads` the supports and loads on the model. `range` compares one colour range across steps. `probe` reads an actual surface node or element and locates Min and Max. `scale` shows the deformation factor. `deformation` has the 1× and Auto actions. `fit` fits the full model and keeps its direction. `report-view` restores the report camera. `views` offers exact ±X, ±Y, ±Z and Isometric views. `help` expands the mouse actions and shortcuts. Use `fit` for a long model. Use `views` for an end face. Use `report-view` for a marked region. Use `help` for camera changes. The camera toolbar shows only the named controls. `data-controls=""` shows none. | Each control that can change the view of the run |
| `data-step="N"` | The step that shows first, from 1. | The last step |
| `data-scale="X"` | The deformation scale. `1` is the true shape, `0` the undeformed shape. | `0` for a topology run, `1` for a multibody run, else the automatic scale when the deformation controls show, else `1` |
| `data-box="X0,Y0,Z0,X1,Y1,Z1"` | The box that the view shows first, in the units of the model. A flat box with no motion in z shows from the top: put Z0 and Z1 on the top face of the model, for example z = 1 for a 2D OpenFOAM mesh from z = 0 to 1. | The whole model |
| `data-marks="N:X,Y,Z TEXT;X,Y,Z TEXT"` | The marks on the model, with `;` between them. A mark with `N:` shows only at the step N, for example the front of a flow at each step of a time slider. The point is in the units of the undeformed model. | No mark |

For example, the pressure on an airfoil at 2 flap angles in the bar of the viewer, with the plot of the pressure coefficient along the chord:

```
<label data-bar>Flap angle <select>
<option data-run="run-000004" data-fields="p,U magnitude" data-controls="" data-box="-0.2,-0.4,1,1.3,0.4,1">10 degrees up</option>
<option data-run="run-000005" data-fields="p,U magnitude" data-controls="" data-box="-0.2,-0.4,1,1.3,0.4,1" selected>0 degrees</option>
</select></label>
<!--curve cp.csv-->
```

A slider of the time of a run with 3 steps, with the front of the water marked at each step:

```
<label data-bar>Time <input type="range" min="1" max="3" value="3" data-run="run-000006" data-fields="alpha.water" data-controls="" data-labels="0 s|0.1 s|0.2 s" data-marks="1:0.11,0,1 front;2:0.25,0,1 front;3:0.4,0,1 front"></label>
```

Use `range` for comparisons across steps, `probe` for local values and extremes, and `scale` for a scaled shape.
Reason: the column, heat-fin and crack runs exposed ambiguous colours, local values and deformation factors.

A curve has the named `values` control. It gives a sample cursor and a table of original values, with gaps.
A curve shows Show values when no `data-controls` is around its mark.
Use `<section data-controls="">` around a curve mark to hide it, and `data-controls="values"` to show it. Show all controls restores it.
Reason: heat-fin and frequency curves require exact values from each series, without values from rounded plot coordinates.

### Optional CFD controls

Use these controls only when the analysis needs them.
Put them inside a `section data-cfd`.
The controls use the shared menu, segmented control, switch, value readout, and shortcut row.
Each control wrapper uses `data-cfd-control="NAME"` for its name from the table.
No `data-controls` shows every applicable control. An explicit `data-controls` shows exactly the named controls.
An empty value hides the controls. "Show all controls" temporarily shows every applicable control.
The names below join `steps`, `deformation`, and `loads` in that attribute.
Reason: each view must use the same rule for control visibility.
Reason: the flap report hid useful velocity data and did not connect its plots to the selected run.

`fh report --cfd DATA.json` embeds the source data for these controls.
The flap case writes this file through `curves.py`.
The file contains final field limits, ordered upper and lower Cp samples, coefficients, histories, and cell velocity traces.
No control fabricates absent data. Old reports retain their original controls and contour limits.
Reason: absent data must not produce a false result.

| Name in `data-controls` | HTML attribute | Use |
|---|---|---|
| `cfd-field` | Radio input `data-cfd-field`, value `p` or `U magnitude` | Choose pressure or speed in 1 action. Put the labels in a shared `segmented` component. |
| `cfd-mesh` | Checkbox `data-cfd-mesh` | Show mesh edges over a CFD contour. Omit `checked` for a clear field. |
| `cfd-flap` | Select `data-cfd-flap` | Connect the field, lift, moment, Cp, history, and flow to one solved angle. The first select supplies each option through `data-run` and `data-angle`. |
| `cfd-load` | Container `data-cfd-load="lift"` or `"moment"`, with a curve and an output | Show the selected coefficient and reference on separate scales. |
| `cfd-chord-station` | Range input `data-cfd-station`, inside `data-cfd-cp` | Show separate upper and lower −Cp values and the same station on the airfoil. The readout identifies interpolation. Use a labelled output. |
| `cfd-force-interval` | Range input `data-cfd-interval`, inside `data-cfd-history` | Show the iteration interval and the force range. Keep mesh convergence separate. |
| `cfd-streamlines` | Checkbox `data-cfd-streamlines`, with a `data-cfd-flow` figure | Show steady planar cell velocity traces from upstream seeds. A trace stops at a boundary or finite limit. |
| `cfd-flap-seed` | Checkbox `data-cfd-seed` | Add the source trace from a seed near the flap. |

A history container can contain `data-cfd-angle="ANGLE"` sections with static CSV plots and a `data-cfd-history-absent` paragraph.
Use `<!--cfd-curve NAME.csv-->` for these plots to keep full SVG samples in a small report.
Each residual plot uses initial residuals on a log axis. Absent samples form gaps.
A streamline figure states its source.
Without sufficient source data, it shows an explicit absent-data message.

The view also accepts `data-range="LOW,HIGH"`, `data-mesh="false"`, and `data-quantity="velocity"` or `"kinematic-pressure"`.
The range and quantity apply only to the first named field. The report source must justify them.
Reason: one field name alone does not establish its physical unit.

## Curves and fits

A curve is a CSV file of x-y values. `fh report ... --curve FILE` draws it as a plot at its mark in your text (see "The story of a report"), and `--curve` needs no `--run`. Write a curve for each result that is a line, not a field: a mesh study, a history over the time or the iterations, a response over the frequency.

```
# title: Dam break, the surge front
# y: Z = x / a [1]
# log: x
# points measured: Martin and Moyce (1952), Table 2
# note: The front is the largest x of a cell centre with alpha.water > 0.5.
T = t sqrt(2 g / a) [1],model,measured
0.28,1.00,
1.19,,1.44
```

- The header line is the x quantity, then the name of each series (at most 8). Give each quantity its unit in `[ ]`, and `[1]` when it has no unit. An empty cell has no value.
- `points NAME: SOURCE` shows the series NAME as points, with its source under the plot. Use it for the reference values. The other series are lines.
- `log` makes the x axis, the y axis or both (`x y`) logarithmic. `title` and `note` are optional.
- `gaps: break` makes an empty cell break the line of its series. Use it for absent samples, for example an absent residual. A sample between 2 empty cells shows as a point. Without it, a line joins the values of its series across empty cells.
- Write the file with an `fh-python` script from the tables of the solver. A case can also write it, for example `system/front.awk` of the validation case `dam-break-vof`.

`fh fit MODEL CURVE.csv [--series NAME] [-o FIT.csv]` fits a model to 1 series by least squares. It prints the parameters with their units, R^2, the largest residual and the range of the points. Use the parameters only in that range. `-o` writes the curve with the fit as a new series: show it with `--curve FIT.csv`.

| Model | y(x) | Use |
|---|---|---|
| `poly1` to `poly6` | c0 + c1 x + ... + cN x^N | A calibration or a trend |
| `power` | C x^m, fitted to ln y | The Paris law: x = dK, y = da/dN |
| `neo-hooke` | 2 (x - x^-2) C10 | A uniaxial test of rubber: x is the stretch (1 + the engineering strain), y is the nominal stress |
| `mooney-rivlin` | 2 (x - x^-2) (C10 + C01 / x) | The same |

- For `neo-hooke` and `mooney-rivlin`, `fh fit` prints the `*HYPERELASTIC` lines for CalculiX in the units of y. D1 is for a bulk modulus K of 1000 times the shear modulus. A uniaxial test gives no K: if the request gives K, use D1 = 2 / K.
- A uniaxial fit is for uniaxial tension. For a part in biaxial tension or in shear, list the missing tests in the missing details.
- The constants of `mooney-rivlin` give the same stress in ccx: a fit of the ccx curve of 1 C3D8 element to the stretch 3 gave C10 and C01 within 0.2 %. The solver of the fit agrees with the certified values of NIST within 1e-12 (the validation case `fit-nist-pontius`).

## Choose the CalculiX solver

The default solver SPOOLES is direct. Its memory grows faster than the model, and a run that reaches the memory limit stops. `ccx` writes the number of equations of a run in `job.log`, in the folder of `job.dat` that `fh run` prints: the line `number of equations`, then the value on the next line. The sizes below are measured on a Mac with 16 GB of RAM (13.6 GiB limit) for linear static steps. Scale them with the RAM.

- Up to 300000 equations: use SPOOLES. It is exact, and a run takes less than 2 minutes.
- Compact solid above 300000 equations: write `*STATIC, SOLVER=ITERATIVE CHOLESKY`. It needs 10 times less memory. SPOOLES stops at about 800000 equations. CHOLESKY solved 4.6 million equations in 7 GiB, in 21 minutes. `SOLVER=ITERATIVE SCALING` is 2 times slower but more exact.
- Thin part (smallest side less than 1/50 of the largest side): use SPOOLES, up to 1.5 million equations. Above that, use a coarser mesh. An iterative solver is slow on a thin part, and it can give a wrong displacement across the thickness.

An iterative solver can end with "converged" and a wrong result, with no warning. Thus check it before you trust it:

1. Make a coarse mesh of at most 150000 equations. Request `*NODE PRINT, NSET=NALL` with `U`.
2. Solve it with SPOOLES and with the iterative solver. Compare `U` of each node with a short `fh-python` script.
3. If the largest difference of one component is more than 1% of the largest displacement, use SPOOLES.

## fh mesh sets

Each selection is a plane (`x:V`, `y:V` or `z:V`), a box (`box:X0,Y0,Z0,X1,Y1,Z1`) or, for `--nodes`, a point (`point:X,Y,Z`).

- `--face NAME=SELECTION` makes the node set `NAME` and the surface `NAME` of the faces of the part in the selection. Use the node set in `*BOUNDARY` and `*CLOAD`. Use the surface in `*DSLOAD`.
- `--nodes NAME=SELECTION` makes a node set.
- `--refine NAME=H` makes the elements at the `--face` set `NAME` the size H. They grow to `--size` away from the set. Use it at the small load sets and support sets, for example the pads of a pin, with H of about 1/10 of the width of the set. Do not refine a large set, such as a whole fixed end: it adds many equations and does not change the result.
- `--volume NAME=box:...` makes an element set of the elements in the box. Use it for a part with several materials.
- `NALL` is the set of all nodes. `ALL` is the set of all elements.
- A positive pressure in `*DSLOAD` pushes into the body: `NAME, P, 1.0` is 1 MPa on the face.
- `*FILM` refuses a surface name. Write 1 line `E, Fn, SINK, H` for each line `E, Sn` of the surface.

## Shells

Use shells for a thin part.

- Draw the mid-surface in build123d, not the solid. A flat plate is a `Face`. A curved shell is `Face.extrude(edge, direction)` of an arc. Write the STEP as before.
- Mesh: `fh mesh part.step --shell --size H -o mesh.inp ...` writes S6 elements (6-node shell triangles). With `--shell`, `--face NAME=SELECTION` selects edges of the surface, and `--volume NAME=box:...` selects surfaces.
- Section: `*SHELL SECTION, ELSET=ALL, MATERIAL=STEEL`, then the thickness on the next line. A shell node has 6 degrees of freedom: `FIXED, 1, 6` clamps it.
- `*DSLOAD` on an edge surface: `NAME, P, 1.0` is a pressure of 1 MPa on the edge face. The force is P times the thickness times the length of the edge.
- On a symmetry plane, fix the normal translation and 1 rotation about an axis in the plane. For the plane x = c, fix DOF 1 and 5. Do not fix 2 rotations at the nodes of 1 plane: ccx then made the roof 8 times too stiff.

## Composite laminates

- Ply: `*ELASTIC, TYPE=ENGINEERING CONSTANTS`, with `E1, E2, E3, nu12, nu13, nu23, G12, G13` on the first line and `G23, 0.0` on the second. Axis 1 is the fibre.
- Ply angle θ: 1 `*ORIENTATION` for each angle, `cos θ, sin θ, 0, -sin θ, cos θ, 0`. The first point is on axis 1.
- Composite shell (`S8R` or `S6` only): `*SHELL SECTION, ELSET=..., COMPOSITE`, then 1 line for each layer: `thickness, , material, orientation`. The first layer is on the negative side of the normal.
- A composite shell is too stiff when the laminate is thick. Its deflection was 1 % low at span/thickness 50, 6 % low at 20 and 16 % low at 10. Its transverse shear stress was wrong by a factor of up to 3.
- Thus, below a span/thickness of 50, or for the shear stress between the plies, use `C3D20R` with 3 elements across each ply and 1 `*SOLID SECTION, ..., ORIENTATION=...` for each ply angle.
- `*EL PRINT` gives the stress of an element with an orientation in the axes of the ply (11 is along the fibre) and adds the name of the orientation at the end of each line. Use these stresses for a ply failure index, for example Tsai-Wu or Hashin, in an `fh-python` script. `GLOBAL=YES` gives x, y, z.

This setup agrees within 0.2 % with the exact 3D solution of Pagano (1970) for [0/90/0] plates under a sine load, with shells at span/thickness 100 and with 3D layers at 4 (the validation case `laminate-pagano`).

## Cracks and fatigue

For a stress intensity factor K_I, put the crack in the mesh and get K_I from the displacements of the crack face.

1. Model: plane strain (`CPE6`, `CPE8`) or plane stress (`CPS6`, `CPS8`). When the crack is on a symmetry plane, model 1 side: fix the normal displacement on the ligament and keep the crack face free. Write the nodes and the elements with an `fh-python` script.
2. Mesh: a ring of 6-node triangles about the tip, then rings of 8-node quadrilaterals. In each triangle, put the mid-side nodes of the 2 edges from the tip at 1/4 of the edge from the tip. Make the edge length L from the tip about 1/50 of the crack length.
3. K_I: on the crack face, B is the node at L/4 from the tip and C is the node at L. With the symmetry plane, K_I = c (4 v_B - v_C), c = E'/4 sqrt(2 pi / L). v is the displacement normal to the crack, E' = E / (1 - nu^2) in plane strain and E' = E in plane stress. Without the symmetry plane, use the opening of the 2 faces for v and E'/8 for E'/4.
4. Add a node K with no element. For a crack along x, write `*EQUATION`, the line `3`, and the line `K, 2, 1.0, B, 2, -4c, C, 2, c` with the numbers. Then `*NODE PRINT` of K writes K_I as U2 in `job.dat`, in MPa mm^0.5. 1 MPa m^0.5 is 31.62 MPa mm^0.5.
5. Fields for the page: write `*NODE FILE, OUTPUT=3D` in the first step. Without `OUTPUT=3D`, ccx writes 2D elements, and the page does not show them.
6. Fatigue life: solve 4 or more crack lengths. Integrate the Paris law da/dN = C dK^m from the first length to the last length in an `fh-python` script. Give C with its units.
7. `*CRACK PROPAGATION` does not model the crack. It takes K from the stress of the part without the crack, with fixed shape factors. Do not use it for K of a crack in the mesh.

This setup agrees within 0.2 % with the handbook value of K_I of a centre crack in a plate (the validation case `fracture-centre-crack`).

## Submodels

For the peak stress at a small detail of a large part, solve a coarse global model of the part, then a fine submodel of the region at the detail. The submodel gets the displacement of the global model on its cut boundary.

1. Global model: write `global.inp`, a complete model with `*NODE FILE, OUTPUT=3D` and `U` in its step. Without `OUTPUT=3D`, a 2D submodel stopped with `*ERROR in dsort`.
2. Submodel: write `job.inp` beside `global.inp`, with a fine mesh of the region and a node set `CUT` of the cut boundary. Add `*SUBMODEL, TYPE=NODE, INPUT=global.frd` with the line `CUT` before the first step. In the step, write `*BOUNDARY, SUBMODEL, STEP=1` with the line `CUT, 1, 2` (`CUT, 1, 3` in 3D), and no other load at the cut.
3. Solve: `fh run job.inp`. `fh` copies `global.inp` and its `*INCLUDE` files, solves `global.inp`, then solves `job.inp` in the same folder.
4. Cut: put the cut far from the detail, for example at r = 2a for a hole of radius a. A cut at r = 1.5a gave an error of 1 %. Near the cut, the submodel has the error of the global model: read its results away from the cut.
5. Check: the submodel is only as good as the displacement of the global model at the cut. Solve 2 global meshes and compare the result of the submodel.

This setup agrees within 0.03 % with the Kirsch solution for the stress at a hole in a plate in tension, from a global model of 16 elements that is 4.5 % low (the validation case `submodel-kirsch-hole`).

## Rubber

Use `*HYPERELASTIC` for rubber. It turns on `NLGEOM`.

- Constants: `NEO HOOKE` takes C10 and D1. `MOONEY-RIVLIN` takes C10, C01 and D1. `OGDEN, N=3` takes mu1, alpha1, mu2, alpha2, mu3, alpha3, D1, D2 on line 1 and D3 on line 2.
- An Ogden fit in a paper often has the energy sum mu_p / alpha_p (l1^alpha_p + l2^alpha_p + l3^alpha_p - 3). ccx has 2 mu_i / alpha_i^2 in place of mu_p / alpha_p. Thus write mu_i = mu_p alpha_p / 2. The shear modulus at small strain is mu0 = sum mu_i, or 2 (C10 + C01).
- Write D1 = 2 / K with K of about 10^4 mu0. Do not write D1 = 0: ccx then uses K = 20 mu0, with only a warning. In the tube below, K = 20 mu0 gave an error of 6 % and K = 100 mu0 an error of 1.6 %. D2 = D3 = 0 has no effect.
- Use `CPE8R`, `CAX8R` or `C3D20R`. A `C3D10` mesh with 5 elements through the wall stopped. With 8 elements, the error was 0.7 %.
- A pressure load stops at a maximum of the pressure, for example in an inflated tube or a balloon. ccx has no arc length method. Thus move the face: `*TRANSFORM, NSET=INNER, TYPE=C` with the axis, then `INNER, 1, 1, UR` and `INNER, 2, 2, 0.0` in `*BOUNDARY`.
- Then get the force from `*NODE PRINT, NSET=..., GLOBAL=YES` with `RF`. Without `GLOBAL=YES`, ccx writes the forces of these nodes in the cylindrical system. Do not put the nodes of a symmetry plane in the set, because their force also holds the symmetry force.
- Ogden with 2 equal stretches, as in a sphere or in equibiaxial tension, can stop with "too many cutbacks". Neo-Hooke on the same mesh converged. Then change the first increment or the mesh, and compare the results.

This setup agrees within 0.02 % with the exact inflation of a rubber tube past its pressure maximum (the validation case `hyperelastic-tube`).

## Random vibration

For a random vibration request, for example a part on a shaker with an acceleration PSD, write 2 steps in `job.inp`. `fh run` then writes the RMS of the response.

1. Modes: `*FREQUENCY, STORAGE=YES`, with the modes up to at least 2 times the highest frequency of the PSD. `STORAGE=YES` keeps the modes for the next step.
2. PSD: an `*AMPLITUDE` with the square root of the PSD, in mm/s2 per sqrt(Hz), at each frequency. For 0.04 g^2/Hz, it is sqrt(0.04) x 9806.65 = 1961.33. `ccx` interpolates the amplitude linearly, thus give a point at least each 10 % of frequency where the PSD slopes.
3. Response: `*STEADY STATE DYNAMICS` with the line `F_LOW, F_HIGH, N, 1.`, then `*MODAL DAMPING` with the line `1, LAST_MODE, ZETA`, then `*BASE MOTION, DOF=3, TYPE=ACCELERATION, AMPLITUDE=PSD` with the support set on the next line. The support set has `*BOUNDARY` in that direction.
4. Points: `ccx` puts N points in each interval between F_LOW, each natural frequency and F_HIGH. Make the spacing at most zeta f1 / 2, with f1 the lowest natural frequency in the band. A spacing of 1.2 zeta f1 gave an RMS 0.5 % too high.
5. Output: `*NODE PRINT` of `U` for each set to know, in the steady state step. A `*NODE FILE` of the modes step stays active, and it writes each frequency to the `.frd` file (70 MB for 2000 nodes). Add `*NODE FILE, FREQUENCY=0` and the line `U` to the steady state step to stop it.
6. Results: `ccx/rms.txt` has, for each node of each print set, the RMS of `U1` to `U3` and of the acceleration `A1` to `A3`. With `*BASE MOTION`, `U` is the absolute displacement: it holds the motion of the base. Thus use `A`. `ccx` prints no stress in this step. Give the peak as 3 times the RMS (3 sigma).
7. Hand check: Miles' equation, A_rms = c sqrt(pi/2 f1 Q W), with Q = 1 / (2 zeta) and W the PSD at f1 in g^2/Hz. c is 1 for a mass on a spring, and 1.566 for the tip of a uniform cantilever. `job.dat` has the effective modal mass of each mode.

This setup agrees within 0.1 % with the modal sum of the beam theory for a cantilever on a shaker (the validation case `random-vibration-cantilever`). Miles' equation gave 1.9 % less there.

## Output requests

Put the output requests inside the `*STEP`, before `*END STEP`.

Text values, in `job.dat` (read these):

```
*NODE PRINT, NSET=FIXED, TOTALS=YES
RF
*NODE PRINT, NSET=TIP
U
*EL PRINT, ELSET=ALL
S
```

- `TOTALS=YES` adds the sum of the forces of the set (`total force`). Use it to check a reaction.
- `*NODE PRINT` gives `U`, `RF` and `NT`, but not the stress. `*EL PRINT` gives `S`, `E` and `PEEQ` at the integration points. Use `*EL PRINT` for a small element set only: the file has a line for each integration point.
- A frequency step writes its eigenvalues and frequencies to `job.dat`.

Fields for the page, in the `.frd` file:

```
*NODE FILE
U, RF
*EL FILE
S, E
```

The page shows every field that these requests write, at each increment that they write. `FREQUENCY=N` writes each N-th increment: use it in a step with many increments, so that the result files stay small. Without the requests, the page has no field to show.

For 2D elements (`CPE`, `CPS`, `CAX`), write `*NODE FILE, OUTPUT=3D` in the first step. ccx ignores `OUTPUT=3D` in a later step and writes 2D elements, and the page does not show them. The file requests of step 1 stay on in the next steps.

For a model with `*MODEL CHANGE, TYPE=ELEMENT`, write `*NODE FILE, OUTPUT ALL` in the first step. ccx writes the mesh only at the first output: without `OUTPUT ALL`, it has only the elements in the model at that time. With it, the page shows in each step only the elements in the model in that step.

## Supports and loads

A wrong support face or a wrong load direction gives a result that looks correct. Thus, when the request does not give a support or a load, put your choice in the list of missing details.

## Example: a cantilever with a pressure on the top face

`fh mesh part.step --size 2.5 -o mesh.inp --face FIXED=x:0 --face TOP=z:10 --nodes TIP=x:100`

```
*INCLUDE, INPUT=mesh.inp
*MATERIAL, NAME=STEEL
*ELASTIC
210000.0, 0.3
*SOLID SECTION, ELSET=ALL, MATERIAL=STEEL
*STEP, NAME=LOAD
*STATIC
*BOUNDARY
FIXED, 1, 3
*DSLOAD
TOP, P, 1.0
*NODE PRINT, NSET=FIXED, TOTALS=YES
RF
*NODE PRINT, NSET=TIP
U
*NODE FILE
U
*EL FILE
S
*END STEP
```


## Optional controls of the live page

The live page uses named controls from the shared UI layer. Its `data-controls` attribute on `body` selects only the named optional controls.
The Runs list is page navigation, not an optional control. It is always on the page.
For example, `&controls=run-search,files,mesh-compare` shows the controls for a mesh study.
The `controls` URL parameter sets that attribute for a live page link.
Without `data-controls`, the page offers every applicable control. With `data-controls`, it offers exactly the named optional controls.
An empty attribute hides every optional control. These page controls apply to a live chat, not to a static report.
Keep the `token` and `run` parameters. Never put the token in a report or an artifact record.

| Name | Use |
| --- | --- |
| `command-state` | Show Active, Done, Failed, or Stopped, with the elapsed command time. Use it for a solve or a report command. |
| `prepare-rerun` | After a stop, fill the composer with a request for the retained input of the selected run. The person must send the request. |
| `files` | Offer the retained input, result files, and the latest registered report of the selected run. Use it when the person must inspect the input or report. |
| `run-search` | Show a search box above the Runs list for run IDs and source paths. Use it when an analysis has several runs. |
| `mesh-compare` | Choose mesh A and mesh B from Runs. Show undeformed meshes with a linked camera and zoom. Use it to inspect mesh changes. |

A mesh pair gives no convergence result. It compares no contour field, unit, time step, or independent deformation scale.
A load error names its side. The other side stays open. Uncheck Link views for separate camera poses.
The report command retains a report copy for each named run. Open report uses a sandbox with no daemon access.
Reason: scout runs R5 to R9 showed repeated run names, hidden report paths, and unclear command and stop state.
