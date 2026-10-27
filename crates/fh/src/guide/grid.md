# Heat conduction with the built-in grid solver

Use the built-in grid solver first for linear heat conduction in parts that are boxes. It needs no CAD, no mesh and no CalculiX, and a model of 100,000 unknowns solves in about 1 s. Write `CASE/grid.toml` and solve it with `fh run CASE`.

Use CAD, `fh mesh` and CalculiX (`*HEAT TRANSFER`) when the model has a curved or sloped face, contact, radiation, a material that changes with the temperature, or a stress. The grid solver refuses those inputs. It never approximates them.

1. Top: `units = "SI-mm"` or `"SI-m"`. All numbers are in these units. In SI-m, a conductivity is in W/(m K), a flux in W/m2, a film coefficient in W/(m2 K), a density in kg/m3, a specific heat in J/(kg K) and a source in W/m3. In SI-mm, a conductivity is in mW/(mm K), a flux in mW/mm2, a film coefficient in mW/(mm2 K), a density in t/mm3, a specific heat in mJ/(t K) and a source in mW/mm3. For example, steel in SI-mm has `density = 7.85e-9` and `specific_heat = 4.6e8`, not 7850 and 460.
2. `[grid]`: `max_cell` (the largest cell edge), `max_growth` (the largest size ratio of 2 adjacent cells, 1.3 by default), `min_cells` (the fewest cells through each part in each direction, 2 by default) and `symmetry` (for example `["x", "y"]`).
   - With `symmetry`, write the full model. The solver checks that the model is symmetric about the middle plane of each axis that you name, then solves only 1 half (or quarter). The results show the half. A probe in the other half reads its mirror point.
3. `[material.NAME]`: `conductivity` (1 number, or 3 numbers for x, y and z), and `density` with `specific_heat` for a transient step.
4. `[part.NAME]`: `material`, and `x`, `y`, `z`, each the range of the box, for example `x = [0.0, 10.0]`. Where 2 parts overlap, the part with the larger `priority` (a whole number, 0 by default) holds the cells. Space with no part is empty.
5. `[face.NAME]`: a set of the outer faces of the model. `side` (`"x-"` ... `"z+"`, or a list) keeps the faces whose outward normal is that direction. `box = [x0, y0, z0, x1, y1, z1]` keeps the faces whose centre is in the box. `parts` keeps the faces of those parts. A face set with no key has all outer faces. An outer face with no load is adiabatic.
6. `[[load]]`, 1 for each load: `temperature` with `face`, `flux` (into the part) with `face`, `convection` (the film coefficient) and `sink` with `face`, or `source` (the heat in each unit of volume) with `part`.
7. `[step]`: `type = "steady"`, or `type = "transient"` with `end`, `dt`, `initial` (the temperature at time 0) and `output_every` (the number of time steps between 2 outputs, 1 by default). The transient step is backward Euler: give at least 10 steps over the time that the result needs.
8. `[probe.NAME]`: `point = [x, y, z]`. The probe gives the temperature at that point.

`fh run` writes the temperature `NT`, the heat flux `HFL` and the part number of each cell for the page. In the run folder, `grid/probes.txt` has the time and each probe, `grid/energy.txt` has the heat flows of the energy balance, and `grid/log.txt` has the solver lines and the warnings. Read the warnings: an energy imbalance above 1e-6 of the gross flow is a warning.

Each face of a part is on a plane of the grid, so a box of any size is exact. Do a mesh study with 2 values of `max_cell`. Near the edge of a part that touches a larger part, the temperature converges slowly, because the heat flux is singular there. Put the probes for a check away from those edges.
