# Heat conduction, stress, unit cells, DC conduction, transmission lines and electromagnetics with the built-in grid solver

Use the built-in grid solver first for:

- heat conduction, steady or transient, in parts that are boxes, or prisms of a polygon or a circle (with holes) along 1 axis, also with a radiation to the ambient and with a conductivity and a specific heat that change with the temperature. Examples: layers and laminates, plates with holes, pipes, fins, and extruded sections; hot parts in vacuum or in still air (space hardware, furnaces, lamps, heaters), and materials over a large range of temperature.
- linear static stress and deformation in the same parts, with isotropic or orthotropic materials, from supports, pressures, forces, gravity and a temperature change (a uniform temperature, or the steady heat solve of the same case). Examples: frames and laminates of boxes, plates with holes, tubes under pressure, round bars, and stiffened extruded sections.
- DC current conduction and electrostatics in the same parts: the resistance between face sets, the current density and the Joule heat, the temperature of the Joule heat (also with an electric conductivity that changes with the temperature) and its thermal stress, and the capacitance matrix of conductors in a dielectric. Examples: resistors, bus bars, fuses, heaters, traces with vias, contact spots, electrodes and capacitors. It needs no GetDP. See "DC conduction and electrostatics" below.
- the homogenized (effective) material of a periodic unit cell: the conductivity, the stiffness and the expansion of a composite, a laminate, a lattice, a foam or a woven or stacked structure that repeats in x, y and z (`type = "homogenize"`, see "Unit cells" below).
- linear frequency-domain electromagnetics in the same shapes: the S-parameters of waveguides, transmission lines, steps, vias, filters and transitions with wave ports, and the resonant frequencies and Q of cavities. It needs no gmsh and no GetDP. See "Electromagnetics" below.

It needs no CAD, no mesh and no CalculiX, and a model of 100,000 unknowns solves in about 1 to 5 s. Write `CASE/grid.toml` and solve it with `fh run CASE`.

Use CAD, `fh mesh` and CalculiX when the model has a curved or sloped face that is not a prism along the axis (for example a sphere, a fillet or a cone), contact, radiation between surfaces (with view factors), plasticity or another nonlinear elastic material, large deformation, vibration or buckling, or elastic constants that change with the temperature. The grid solver refuses those inputs. It never approximates them.

1. Top: `units = "SI-mm"` or `"SI-m"`. All numbers are in these units. In SI-m, a conductivity is in W/(m K), a flux in W/m2, a film coefficient in W/(m2 K), a density in kg/m3, a specific heat in J/(kg K), a source in W/m3, a modulus and a pressure in Pa, and a length in m. In SI-mm, a conductivity is in mW/(mm K), a flux in mW/mm2, a film coefficient in mW/(mm2 K), a density in t/mm3, a specific heat in mJ/(t K), a source in mW/mm3, a modulus and a pressure in MPa, and a length in mm. For example, steel in SI-mm has `density = 7.85e-9`, `specific_heat = 4.6e8` and `youngs_modulus = 210e3`, not 7850, 460 and 210e9.
2. `[grid]`: `max_cell` (the largest cell edge), `max_growth` (the largest size ratio of 2 adjacent cells, 1.3 by default), `min_cells` (the fewest cells through each part along each axis of the grid, 2 by default), `axis` (the extrusion axis of the polygon and circle parts: `"x"`, `"y"` or `"z"`, `"z"` by default) and `symmetry` (for example `["x", "y"]`).
   - With `symmetry`, write the full model. The solver checks that the model is symmetric about the middle plane of each axis that you name, then solves only 1 half (or quarter). The results show the half. A probe in the other half reads its mirror point. A model with a polygon or a circle part has no `symmetry`.
3. `[material.NAME]`: `conductivity` (1 number, or 3 numbers for x, y and z), and `density` with `specific_heat` for a transient step. A `conductivity` or a `specific_heat` that changes with the temperature is a table, for example `conductivity = { temperature = [300.0, 500.0, 700.0], value = [14.0, 17.5, 20.0] }`: 2 or more temperatures in increasing order, with linear interpolation between them and the end value beyond the ends. The table conductivity is the same along x, y and z. `density` stays 1 number. For a static step:
   - isotropic: `youngs_modulus` and `poisson_ratio`, 1 number each.
   - orthotropic, with the material axes on x, y and z: `youngs_modulus` (x, y, z), `poisson_ratio` (xy, xz, yz) and `shear_modulus` (xy, xz, yz), 3 numbers each. `poisson_ratio` xy is −(strain y) / (strain x) under a stress in x.
   - `expansion`: the expansion coefficient, 1 number, or 3 numbers for x, y and z (0 by default).
   - `density`, for gravity.
   - `conductivity`, only when the step has heat loads.
4. `[part.NAME]`: `material`, and the shape:
   - A box: `x`, `y`, `z`, each the range of the box, for example `x = [0.0, 10.0]`.
   - A prism: `polygon = [[a, b], [a, b], ...]` (3 or more corners) or `circle = [a, b, radius]`, then `holes` (optional, a list of polygons and circles), and the range along the axis only, for example `z = [0.0, 2.0]`. The coordinates a and b are across the axis: x and y for the axis z, y and z for the axis x, z and x for the axis y.
   - Where 2 parts overlap, the part with the larger `priority` (a whole number, 0 by default) holds the overlap. Space with no part is empty.
5. `[face.NAME]`: a set of the outer faces of the model. `side` (`"x-"` ... `"z+"`, or a list) keeps the faces whose outward normal is that direction. `box = [x0, y0, z0, x1, y1, z1]` keeps the faces whose centre is in the box. `parts` keeps the faces of those parts. `circle = [a, b, radius]` keeps the faces on that circle of a polygon or circle part, for example the wall of a hole. A face set with no key has all outer faces. An outer face with no load is adiabatic.
6. `[[load]]`, 1 for each load. Heat loads: `temperature` with `face`, `flux` (into the part) with `face`, `convection` (the film coefficient) and `sink` with `face`, `emissivity` (above 0, at most 1) and `ambient` with `face`, or `source` (the heat in each unit of volume) with `part`.
   - `emissivity` is a radiation q = emissivity · sigma · (T^4 − ambient^4) to surroundings that are much larger than the face, for example a part in a vacuum chamber, in space or in a furnace. With a radiation, give all temperatures in kelvin. The solver uses the Stefan-Boltzmann constant sigma of `units`. The radiation between 2 faces of the model is not in the grid solver.
   - A radiation or a table makes the solve nonlinear: the solver does Newton iterations (at each time step of a transient step), and the log has the number of iterations. Each iteration is 1 linear solve, or a few (GMRES) with a conductivity table. A steady solve of the validation cases took 4 or 5 iterations, and a transient fin of 8,100 unknowns with tables took 2 or 3 at each time step (100 steps in 21 s). When Newton does not converge, the solver stops with an error: check the units of the temperatures, and in a transient step use a smaller `dt`.

   Loads of a static step:
   - `ux`, `uy` and/or `uz` with `face`: fixed displacement components. A component that you do not give is free. A support on a plane of symmetry is not necessary: the plane holds the normal component.
   - `ux`, `uy` and/or `uz` with `point = [x, y, z]`: fixed components at 1 point of the grid (a corner of a part, or a point where the planes of 2 parts cross). In a model with a polygon or a circle part, the point is a corner of a polygon, or a point of a circle at 0, 90, 180 or 270 degrees from its centre, on a plane at an end of a part along the axis. Use it to stop the rigid motion of a free part, for example in a warpage model. It must take no force: then the stress has no error from the support.
   - `pressure` with `face`: a force on the face, into the part when it is above 0.
   - `traction = [tx, ty, tz]` with `face`: a force per area.
   - `gravity = [gx, gy, gz]`: the acceleration of gravity, for each part with a `density`.
   - `body_force = [fx, fy, fz]` with `part`: a force per volume.
7. `[step]`:
   - `type = "steady"`.
   - `type = "transient"` with `end`, `dt`, `initial` (the temperature at time 0) and `output_every` (the number of time steps between 2 outputs, 1 by default). The transient step is backward Euler: give at least 10 steps over the time that the result needs.
   - `type = "homogenize"`: the effective material of a periodic unit cell (see "Unit cells").
   - `type = "static"`, with `reference_temperature` (the temperature with no thermal strain) for a thermal load. The thermal load is `temperature` (the same temperature at each point), or the heat loads of the case: then the solver first solves the steady temperature, and the stress uses it. The stress does not change the temperature.
8. `[probe.NAME]`: `point = [x, y, z]`. The probe gives the values at that point.
9. `[surface.NAME]`, static step only: `face`, a face set whose faces all have 1 normal direction. The result gives the shape of the surface after the deformation.

The solver refuses a model that can move as a rigid body, and the message names the free motion, for example "turn about z". Add a support, or a point support for a free part.

A static step with flat cells uses the direct solver up to a factor of 2.5e8 entries (2 GB), and multigrid above it. A cell of a box grid is flat when its longest side is more than 2 times its shortest side. A tetrahedron is flat when it is more than 8 times wider across the axis than it is high along the axis. Multigrid can stop with no result on very flat cells. Then put more cells through the thin layer (a smaller `max_cell` or a larger `min_cells`), or make the model smaller.

The person can also solve the `grid.toml` of a heat or an electromagnetic step in the browser, with no `fh run`: the button "Solve grid.toml" of the page of `fh serve` and of a report solves the file in the page and shows the progress, the fields and the probe table. It gives the same probes as `fh run`. The browser solves up to 1,000,000 points on a box grid and 100,000 on a mesh of tetrahedra (the 4 GiB memory of wasm32), `fh run` up to 4,000,000 and 400,000. The browser does not solve a static step. Use `fh run` for it. Give the person the case folder when they want to change and solve the case themselves.

## Results

`fh run` writes the fields for the page, and text tables in the folder `grid` of the run folder. Each table has a header line that starts with `#` and names the columns, and then the rows.

Heat steps: the temperature `NT`, the heat flux `HFL` and the part number of each cell. `grid/probes.txt` has the time and each probe, `grid/energy.txt` has the heat flows of the energy balance: `source`, `flux`, `convection`, `radiation`, `fixed` (the heat that the fixed temperatures put in), `stored` (the rate of the stored heat) and `imbalance`. Heat into the body is above 0.

Static step: the displacement `U`, the stress `S` and the strain `E` (xx, yy, zz, xy, yz, xz), the von Mises stress `MISES`, the principal stresses `SP` (the largest first) and the temperature `NT` at each point. The value at a point is the mean of the cells around it.

- `grid/probes.txt`: 1 row with `NAME.ux`, `NAME.uy`, `NAME.uz`, `NAME.sxx`, `NAME.syy`, `NAME.szz`, `NAME.sxy`, `NAME.syz`, `NAME.sxz`, `NAME.mises` and, with a thermal load, `NAME.t` of each probe. The stress at a probe is the value of the cell that holds it, so put a probe for a stress inside a cell, not on a cell face. On a mesh of tetrahedra, the stress at a probe is the mean of the cells that hold the point. A probe on an outer face of a part is correct. A probe on a face between 2 materials gives the mean of the 2 materials, so put a probe for a stress inside 1 part.
- `grid/surfaces.txt`: 1 row with the shape of each surface, from the displacement w along the outward normal:
  - `NAME.range`: the largest w minus the smallest w.
  - `NAME.flatness`: the range of w after the removal of the plane of the best fit.
  - the curvatures of the best fit of a quadratic, for example `NAME.kxx`, `NAME.kyy` and `NAME.kxy` for a z face, and the principal curvatures `NAME.k1` and `NAME.k2`. A curvature above 0 bulges outward.
  - `NAME.shape`: `convex` (it bulges outward), `concave` or `flat`, from the mean curvature.
- `grid/reactions.txt`: 1 row with `NAME.fx`, `NAME.fy` and `NAME.fz`, the force of the supports of each fixed face set. With symmetry, the force is that of the solved part.
- `grid/stress.txt`: the largest von Mises stress of each part and its point, and `singular`: `re-entrant`, `material-edge`, `support-edge` or `no`. A singular point is a re-entrant edge or corner, an edge where 2 materials meet, or the edge of a face that holds 2 or more components. There, the exact stress is infinite, so the value grows when the cells get smaller. Do not report it as a stress. The columns `regular_*` give the largest value away from those points.

`grid/log.txt` has the solver lines and the warnings. Read the warnings: an energy imbalance or a force imbalance above 1e-6 is a warning, and so is a stress peak at a singular point.

## Mesh study

A model of boxes only is solved on a box grid: each face of a part is on a plane of the grid, so a box of any size is exact. When a part is a polygon or a circle, the solver makes a quality triangle mesh of all footprints, extrudes it through the planes of the axis, and cuts each prism into 3 tetrahedra. The points of a circle are on the circle, with segments of at most `max_cell`, at least 64 segments and a point at 0, 90, 180 and 270 degrees. A heat step uses linear tetrahedra and the direct solver, and it refuses a mesh of more than 400,000 points. Linear tetrahedra need about 2 times smaller cells than the box grid for the same accuracy. A static step adds a point at the middle of each edge (10-node tetrahedra with straight edges). Its solver is direct up to 30,000 unknowns and on flat cells, and else PCG with p-multigrid. It refuses a mesh of more than 250,000 points (about 750,000 unknowns). For both refusals, use a larger `max_cell` or `max_growth`. With 64 segments on the circle, the 10-node tetrahedra give the stress at the edge of a round hole to 0.4 %, and the bending deflection of a round bar to 0.4 % (the validation cases `grid-kirsch-hole` and `grid-round-cantilever`). A point of a circle is a corner of its polygon. On the wall of a hole with a pressure, the stress at such a point changes by a few % with the mesh: read that stress 5 % of the radius inside the wall.

Do a mesh study with 2 values of `max_cell`. Near the edge of a part that touches a larger part, the temperature and the stress converge slowly, because the heat flux and the stress are singular there. Put the probes for a check away from those edges. The cells have no shear locking: 2 cells through a thin layer give its bending to about 1.5 %, also when the cells are much longer than thick.

## Unit cells

Use `[step] type = "homogenize"` to get the effective material of a structure that repeats a unit cell in x, y and z, for example a fibre composite, a laminate, a particle composite, a lattice or a foam of boxes, or the layers and vias of a board. Then use that material in a larger model, in place of the detail.

- The cell is the box around all parts. The parts on 2 opposite sides of the cell must match: copies of the cell side by side fill the space. The solver refuses a cell whose opposite sides do not match, and the message names a point with no partner. Space with no part is void, for example the pores of a foam.
- The case has no `face`, `load`, `probe`, `surface` or `symmetry`. The sides of the cell are periodic.
- Each material of a part needs `conductivity` for the effective conductivity, and `youngs_modulus` and `poisson_ratio` (or the 9 orthotropic constants) for the effective stiffness and expansion. Give `expansion` for the expansion, and `density` and `specific_heat` for their means.
- The parts are boxes, or for the conductivity only, prisms of polygons and circles along `axis` (for example round fibres along z). The stiffness of a cell with a polygon or a circle part is refused: give the fibre as boxes, for example a square of the same area, or use CalculiX.
- The solver is direct, and the log tells the cost of its factor. A box cell of 20,000 points takes about 2 s for the conductivity and 90 s for the stiffness, and a cell of 5,000 points 1 s for the stiffness. The solver refuses a factor of more than 3e11 flops (about 3 minutes).

`fh run` writes:

- `grid/homogenized.txt`: 1 row with `fraction.NAME` (the volume fraction of each material), `kxx` `kyy` `kzz` `kyz` `kxz` `kxy` (the conductivity tensor), `c11` ... `c66` (the stiffness tensor, upper triangle, in the order xx, yy, zz, yz, xz, xy with the engineering shear strains), `ex` `ey` `ez` `nuxy` `nuxz` `nuyz` `gxy` `gxz` `gyz` (the engineering constants of the compliance), `alphaxx` ... `alphaxy` (the expansion), `density` and `specific_heat`.
- `grid/material.toml`: the table `[material.homogenized]` for the `grid.toml` of a larger model, with `conductivity`, `youngs_modulus`, `poisson_ratio`, `shear_modulus`, `expansion`, `density` and `specific_heat`. Its axes are the x, y and z of the cell. A `grid.toml` material is orthotropic in x, y and z: when the cell couples 2 axes (a term above 1e-4 of the largest), that property is not in the file, and the log has a warning.

The conductivity uses 3 solves with a unit mean gradient, the stiffness 6 solves with a unit mean strain, and the expansion 1 solve with a unit temperature change, each with a periodic fluctuation. A box cell holds a laminate exactly. For round fibres on tetrahedra in a cell of side 1, `max_cell = 0.04` gave the conductivity across the fibres to 0.08 % and `max_cell = 0.02` to 0.02 %. Do a mesh study with 2 values of `max_cell`.

## DC conduction and electrostatics

Use `[step] type = "conduction"` for a DC current in conductors, and `type = "electrostatic"` for the field and the capacitance of conductors in a dielectric. The solver solves div(σ grad V) = 0 (or div(ε grad V) = 0) with the elements of a heat step: hex cells on a box grid, linear tetrahedra on a mesh of prisms. Use GetDP (`fh guide electromagnetics`) for eddy currents, magnetic fields and AC currents.

1. Top: `units` gives the unit of the lengths and of the heat loads only. A voltage is in V, a current in A, a charge in C, an electric conductivity in S/m, a resistance in ohm and a capacitance in F, also in SI-mm.
2. `[grid]`: as for heat, with no `symmetry`.
3. `[material.NAME]`: `electric_conductivity` (S/m, 1 number) for each material of a conduction step. Add `conductivity` (the thermal conductivity) for the Joule heat, and `temperature_coefficient` (alpha, 1/K) for a resistivity ρ0 (1 + alpha (T − T_ref)). For an electrostatic step, `relative_permittivity` (1 by default). Space with no part has no field: give the air or the dielectric around the conductors as a part. A conductor of an electrostatic step is a hole or the outside of the dielectric, with a voltage on its face set.
4. `[[load]]`: each load makes 1 terminal: a face set at 1 potential. Give each terminal its own face set. 2 terminals must not touch.
   - `voltage` with `face`: a fixed voltage. Each part needs a voltage on it, on a part that it touches, or on a terminal that it shares with such a part.
   - `current` with `face` (conduction): a total current into the part through the face set, which has 1 unknown voltage.
   - `charge` with `face` (electrostatic): a total charge on a conductor at 1 unknown voltage. A floating conductor has `charge = 0.0`.
   - Heat loads (`temperature`, `flux`, `convection`, `source`) in a conduction step: then the solver also solves the steady temperature with the Joule heat as a source. A face with no heat load is adiabatic.
   - A static step with `voltage` or `current` loads and heat loads: the stress uses the temperature of the Joule heat.
5. `[step]`: `type = "conduction"`, with `electric_reference_temperature` (T_ref) for a `temperature_coefficient`. Then the solver repeats the electric and the heat solves until the Joule power changes by less than 1e-6. A current drive with alpha above 0 can heat with no limit (thermal runaway): the solver stops after 50 iterations with a message. A voltage drive is stable. A static step takes `electric_reference_temperature` too.
6. `[probe.NAME]`: `point`. The probe gives the potential, and the temperature with heat loads.

`fh run` writes in `grid`:

- `terminals.txt`: 1 row with `NAME.v` and `NAME.i` (conduction) or `NAME.q` (electrostatic) of each terminal (the name of its face set), the Joule power `power` (W), and with 2 terminals the resistance `resistance` between them (ohm).
- `conductance.txt` or `capacitance.txt`: 1 row with `A.B`, the matrix I = G V (S) or Q = C V (F) of the terminals: the Maxwell capacitance matrix. The capacitance between 2 conductors of 2 terminals is `A.A`.
- `probes.txt`: 1 row with `NAME.v` and `NAME.t` of each probe.

The page shows the potential `V`, the current density `J` (A/m2) and the Joule power density `JOULE` (W/m3), or the electric field `E` (V/m), and `NT` with heat loads. Each terminal is 1 more solve with the same factor. The resistance of a box grid is exact for a uniform bar (`grid-resistor-bar`), and a coaxial capacitor of circles is within 0.3 % at the default 64 segments (`grid-coax-capacitor`). At the rim of a contact spot and at a sharp edge of an electrode, the current density is singular, and the resistance converges slowly from below: put small parts of the same material at the rim (`grid-holm-spot`, 2 % low with rings 0.02 of the radius from the rim).

## Transmission lines

Use the built-in line solver first for the cross-section of a transmission line with linear materials, for example a coaxial cable, a microstrip, a stripline, a twisted pair seen as 2 round wires, a busbar pair or a cable bundle. It gives the quasi-TEM per-length matrices R, L, G and C of the signal conductors at each frequency, the modes, and the S-parameters of a length of line. It needs no gmsh and no GetDP. A `grid.toml` with a `[line]` table is a line case. For the 3D field of a waveguide, a line of finite length, a via or a cavity, use the section "Electromagnetics". Use GetDP (`fh guide electromagnetics`) for each other electromagnetic problem, for example a time history, a nonlinear material, a coil or a motor. The line solver refuses those inputs with a message that names GetDP.

The physics: C and G come from an electrostatic solve with a complex permittivity (the loss tangent). R and L come from a magneto-quasi-static solve of A_z with the eddy currents in each conductor, so the skin effect and the proximity effect are in R and L. The solver makes the segments on the outline of each conductor 0.25 skin depth long at the highest frequency. The S-parameters come from the chain matrix exp([0 Z; Y 0] length) of the per-length Z = R + j omega L and Y = G + j omega C.

1. Top: `units = "SI-m"` only. The cross-section is in the plane x y.
2. `[grid]`: `max_cell` and `max_growth` (1.3 by default). Use `max_growth = 1.1` or less when the request needs Z0 or C to better than 1 %.
3. `[material.NAME]`: `relative_permittivity` (1 by default), `loss_tangent` (0 by default), `relative_permeability` (1 by default), and for a conductor `electric_conductivity` (S/m) or `perfect_conductor = true`. `roughness` (the RMS surface roughness in m, with `electric_conductivity`) multiplies the loss of that conductor by the Hammerstad factor 1 + (2/pi) atan(1.4 (roughness / skin depth)^2).
4. `[part.NAME]`: `material`, `priority`, and a box (`x`, `y`) or a `polygon` or a `circle`, with `holes`, as for heat. Space with no part is empty and has no permittivity: give the air or the dielectric around the conductors as a part with the lowest priority.
5. `[line]`: `signals` (the conductor parts that carry a signal), `references` (the return conductors, at 0 V), `electric_walls` (the box sides that are perfect conductors at 0 V: `"x-"`, `"x+"`, `"y-"`, `"y+"`; each other side is a magnetic wall, a plane of symmetry or an open side far away), `frequencies` (1 number or a list, in Hz), `length` (m, optional: with it the solver writes the S-parameters) and `reference_impedance` (50 ohm by default). Each conductor part is a signal or a reference. A line needs a reference or an electric wall.

`fh run` writes `grid/line.txt`: the signal of each number, then 1 line for each frequency with R, L, G and C (N x N each, in ohm/m, H/m, S/m and F/m), `eps_eff` and `loss` (dB/m) of each mode, and for 1 signal `z0_re` and `z0_im` (ohm). With `length`, `grid/line.sNp` is the Touchstone file of the 2N ports: ports 1 to N are the near ends of the signals, ports N+1 to 2N the far ends. The page shows the potential `V1`, `V2`, ... of each signal at 1 V and the current density `J1`, `J2`, ... at the highest frequency. `fh run` solves up to 1,000,000 points, the browser up to 250,000.

Do a mesh study with 2 values of `max_cell`. A sharp corner of a conductor makes a singular field: C, L and Z0 converge, but slower than near a round face.

## Electromagnetics

A `grid.toml` with `[step] type = "frequency"` or `"modes"` is an electromagnetic case. The solver finds the electric field E with first-order edge elements on a mesh of tetrahedra (also for a model of boxes only), with the time factor exp(jωt). Use GetDP (`fh guide electromagnetics`) for eddy currents, magnetostatics, the time domain, nonlinear materials, and curved or sloped shapes that are not prisms along the axis. The solver refuses those inputs with a message that names GetDP.

1. Top: `units = "SI-mm"` or `"SI-m"` gives the unit of the lengths only. A frequency is in Hz, a conductivity in S/m, an impedance in ohm, and beta in rad/m.
2. `[grid]`: `max_cell`, `max_growth`, `min_cells` and `axis`, as for heat. No `symmetry`: use a magnetic or an electric wall on the plane of symmetry. Give at least 10 cells in each wavelength in each material at the highest frequency. The solver warns below 8.
3. `[material.NAME]`: `permittivity` (relative, 1 by default), `loss_tangent`, `permeability` (relative, 1 by default) and `conductivity` (S/m, a loss in the volume). Or `perfect_conductor = true` alone: a part of this material has no mesh, and its faces are electric walls. Use it for a signal conductor or a via.
4. `[part.NAME]`: as for heat. `pml = true` makes the part an absorbing layer (a perfectly matched layer): a box on the outside of the model, about 1 wavelength thick, with at least 8 cells through it. It absorbs along each axis on which it is outside the box of the other parts. Its outer faces stay electric walls.
5. `[face.NAME]`: as for heat. A side selects all outer faces with that normal, also the face of a step: add `parts` or `box` to keep only the end of 1 guide.
6. `[[load]]`: each outer face is an electric wall (a perfect conductor, n × E = 0) by default.
   - `wall = "magnetic"` with `face`: n × H = 0. Use it on a plane of symmetry that E is parallel to. A TE10 port needs electric walls on all 4 sides: model the full width of a waveguide with a TE10 port.
   - `wall = "conductor"` with `face` and `conductivity`: a good conductor with the surface impedance of its skin depth. Use it when the conductor is thicker than 3 skin depths.
   - `port = N` with `face` and `mode`: a wave port. Number the ports 1, 2, 3 ... `mode = "te10"` is the TE10 mode of a flat rectangular face of 1 material. `mode = "tem"` with `signal` (the face set of the signal conductor) is the quasi-TEM mode of a line, from 2 electrostatic solves on the port face: a coaxial line, a stripline, or a microstrip in a box. The ground of a TEM port is the electric wall around the face. `deembed` (a length, 0 by default) moves the reference plane of the port into the model by that length.
7. `[step]`: `type = "frequency"` with `start`, `stop` (Hz), `points` and `output_every` (the fields of each Nth frequency). Or `type = "modes"` with `near` (Hz) and `count`: the `count` resonant modes nearest to `near`, of a model with no port. A mode with losses (a conductor wall, a conductivity, a loss tangent) has a Q.

A case has no probes. In the run folder, `grid/sparams.txt` has the frequency and `Sij_re`, `Sij_im`, `Sij_mag` and `Sij_deg` of each pair of ports. `grid/ports.txt` has `beta_k` and `impedance_k` of each port: the wave impedance of TE10, or the line impedance Z0 of TEM. `grid/sparams.sNp` is the Touchstone file. The `sparams.txt` S-parameters are generalized: each port is normalized to the impedance of its own mode. The Touchstone file has the same S-parameters renormalized to 50 ohm at each port, at each frequency. The comment line before each frequency gives the impedances of the modes. The wave impedance of a TE10 port changes with the frequency, so its 50-ohm data is a convention. `grid/modes.txt` has the frequency and Q of each mode. The page shows `E{k}_mag`, `E{k}_re` and `E{k}_im` (the field of the excitation of port k), or `E_mag`, `E_re` and `E_im` of each mode, with the frequency as the time.

The solver of a frequency sweep is direct up to 40,000 points, and GMRES with an auxiliary space preconditioner up to 100,000 points. The browser solves up to 25,000 points with the direct solver and 40,000 with GMRES (the 4 GiB memory of wasm32). A mode solve is always direct. A direct solve of 40,000 points takes about 1 minute and 2 GB, and each port is 1 more solve with the same factor. Each port is 1 GMRES solve. The GMRES iterations grow with the length of the model in wavelengths: each port of a WR-90 guide 60 mm long with 88,479 points took about 340 iterations and 5 minutes of CPU at 10 GHz. A line between 2 conductors (a coaxial line, a stripline) needs many more GMRES iterations than a waveguide.

Do a mesh study with 2 values of `max_cell`. The S-parameters converge at second order. Near the edge of a step or a conductor, the field is singular, and the result converges more slowly.
