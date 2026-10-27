# Heat conduction and stress with the built-in grid solver

Use the built-in grid solver first for parts that are boxes, for:

- linear heat conduction, steady or transient.
- linear static stress and deformation, with isotropic or orthotropic materials, from supports, pressures, forces, gravity and a temperature change (a uniform temperature, or the steady heat solve of the same case).

It needs no CAD, no mesh and no CalculiX, and a model of 100,000 unknowns solves in about 1 to 5 s. Write `CASE/grid.toml` and solve it with `fh run CASE`.

Use CAD, `fh mesh` and CalculiX when the model has a curved or sloped face, contact, radiation, plasticity or another nonlinear material, large deformation, vibration or buckling, or a material that changes with the temperature. The grid solver refuses those inputs. It never approximates them.

1. Top: `units = "SI-mm"` or `"SI-m"`. All numbers are in these units. In SI-m, a conductivity is in W/(m K), a flux in W/m2, a film coefficient in W/(m2 K), a density in kg/m3, a specific heat in J/(kg K), a source in W/m3, a modulus and a pressure in Pa, and a length in m. In SI-mm, a conductivity is in mW/(mm K), a flux in mW/mm2, a film coefficient in mW/(mm2 K), a density in t/mm3, a specific heat in mJ/(t K), a source in mW/mm3, a modulus and a pressure in MPa, and a length in mm. For example, steel in SI-mm has `density = 7.85e-9`, `specific_heat = 4.6e8` and `youngs_modulus = 210e3`, not 7850, 460 and 210e9.
2. `[grid]`: `max_cell` (the largest cell edge), `max_growth` (the largest size ratio of 2 adjacent cells, 1.3 by default), `min_cells` (the fewest cells through each part in each direction, 2 by default) and `symmetry` (for example `["x", "y"]`).
   - With `symmetry`, write the full model. The solver checks that the model is symmetric about the middle plane of each axis that you name, then solves only 1 half (or quarter). The results show the half. A probe in the other half reads its mirror point.
3. `[material.NAME]`: `conductivity` (1 number, or 3 numbers for x, y and z), and `density` with `specific_heat` for a transient step. For a static step:
   - isotropic: `youngs_modulus` and `poisson_ratio`, 1 number each.
   - orthotropic, with the material axes on x, y and z: `youngs_modulus` (x, y, z), `poisson_ratio` (xy, xz, yz) and `shear_modulus` (xy, xz, yz), 3 numbers each. `poisson_ratio` xy is −(strain y) / (strain x) under a stress in x.
   - `expansion`: the expansion coefficient, 1 number, or 3 numbers for x, y and z (0 by default).
   - `density`, for gravity.
   - `conductivity`, only when the step has heat loads.
4. `[part.NAME]`: `material`, and `x`, `y`, `z`, each the range of the box, for example `x = [0.0, 10.0]`. Where 2 parts overlap, the part with the larger `priority` (a whole number, 0 by default) holds the cells. Space with no part is empty.
5. `[face.NAME]`: a set of the outer faces of the model. `side` (`"x-"` ... `"z+"`, or a list) keeps the faces whose outward normal is that direction. `box = [x0, y0, z0, x1, y1, z1]` keeps the faces whose centre is in the box. `parts` keeps the faces of those parts. A face set with no key has all outer faces. An outer face with no load is adiabatic.
6. `[[load]]`, 1 for each load. Heat loads: `temperature` with `face`, `flux` (into the part) with `face`, `convection` (the film coefficient) and `sink` with `face`, or `source` (the heat in each unit of volume) with `part`. Loads of a static step:
   - `ux`, `uy` and/or `uz` with `face`: fixed displacement components. A component that you do not give is free. A support on a plane of symmetry is not necessary: the plane holds the normal component.
   - `ux`, `uy` and/or `uz` with `point = [x, y, z]`: fixed components at 1 point of the grid (a corner of a part, or a point where the planes of 2 parts cross). Use it to stop the rigid motion of a free part, for example in a warpage model. It must take no force: then the stress has no error from the support.
   - `pressure` with `face`: a force on the face, into the part when it is above 0.
   - `traction = [tx, ty, tz]` with `face`: a force per area.
   - `gravity = [gx, gy, gz]`: the acceleration of gravity, for each part with a `density`.
   - `body_force = [fx, fy, fz]` with `part`: a force per volume.
7. `[step]`:
   - `type = "steady"`.
   - `type = "transient"` with `end`, `dt`, `initial` (the temperature at time 0) and `output_every` (the number of time steps between 2 outputs, 1 by default). The transient step is backward Euler: give at least 10 steps over the time that the result needs.
   - `type = "static"`, with `reference_temperature` (the temperature with no thermal strain) for a thermal load. The thermal load is `temperature` (the same temperature at each point), or the heat loads of the case: then the solver first solves the steady temperature, and the stress uses it. The stress does not change the temperature.
8. `[probe.NAME]`: `point = [x, y, z]`. The probe gives the values at that point.
9. `[surface.NAME]`, static step only: `face`, a face set whose faces all have 1 normal direction. The result gives the shape of the surface after the deformation.

The solver refuses a model that can move as a rigid body, and the message names the free motion, for example "turn about z". Add a support, or a point support for a free part.

A static step with flat cells uses the direct solver up to a factor of 2.5e8 entries (2 GB), and multigrid above it. Multigrid can stop with no result on very flat cells. Then put more cells through the thin layer (a smaller `max_cell` or a larger `min_cells`), or make the model smaller.

The person can also solve the `grid.toml` of a heat step in the browser, with no `fh run`: the button "Solve grid.toml" of the page of `fh serve` and of a report solves the file in the page and shows the progress, the fields and the probe table. It gives the same probes as `fh run`. The browser solves up to 1,000,000 points (the 4 GiB memory of wasm32), `fh run` up to 4,000,000. The browser does not solve a static step. Use `fh run` for it. Give the person the case folder when they want to change and solve the case themselves.

## Results

`fh run` writes the fields for the page, and text tables in the folder `grid` of the run folder. Each table has a header line that starts with `#` and names the columns, and then the rows.

Heat steps: the temperature `NT`, the heat flux `HFL` and the part number of each cell. `grid/probes.txt` has the time and each probe, `grid/energy.txt` has the heat flows of the energy balance.

Static step: the displacement `U`, the stress `S` and the strain `E` (xx, yy, zz, xy, yz, xz), the von Mises stress `MISES`, the principal stresses `SP` (the largest first) and the temperature `NT` at each point. The value at a point is the mean of the cells around it.

- `grid/probes.txt`: 1 row with `NAME.ux`, `NAME.uy`, `NAME.uz`, `NAME.sxx`, `NAME.syy`, `NAME.szz`, `NAME.sxy`, `NAME.syz`, `NAME.sxz`, `NAME.mises` and, with a thermal load, `NAME.t` of each probe. The stress at a probe is the value of the cell that holds it, so put a probe for a stress inside a cell, not on a cell face.
- `grid/surfaces.txt`: 1 row with the shape of each surface, from the displacement w along the outward normal:
  - `NAME.range`: the largest w minus the smallest w.
  - `NAME.flatness`: the range of w after the removal of the plane of the best fit.
  - the curvatures of the best fit of a quadratic, for example `NAME.kxx`, `NAME.kyy` and `NAME.kxy` for a z face, and the principal curvatures `NAME.k1` and `NAME.k2`. A curvature above 0 bulges outward.
  - `NAME.shape`: `convex` (it bulges outward), `concave` or `flat`, from the mean curvature.
- `grid/reactions.txt`: 1 row with `NAME.fx`, `NAME.fy` and `NAME.fz`, the force of the supports of each fixed face set. With symmetry, the force is that of the solved part.
- `grid/stress.txt`: the largest von Mises stress of each part and its point, and `singular`: `re-entrant`, `material-edge`, `support-edge` or `no`. A singular point is a re-entrant edge or corner, an edge where 2 materials meet, or the edge of a face that holds 2 or more components. There, the exact stress is infinite, so the value grows when the cells get smaller. Do not report it as a stress. The columns `regular_*` give the largest value away from those points.

`grid/log.txt` has the solver lines and the warnings. Read the warnings: an energy imbalance or a force imbalance above 1e-6 is a warning, and so is a stress peak at a singular point.

## Mesh study

Each face of a part is on a plane of the grid, so a box of any size is exact. Do a mesh study with 2 values of `max_cell`. Near the edge of a part that touches a larger part, the temperature and the stress converge slowly, because the heat flux and the stress are singular there. Put the probes for a check away from those edges. The cells have no shear locking: 2 cells through a thin layer give its bending to about 1.5 %, also when the cells are much longer than thick.
