# Electromagnetics with GetDP

For the cross-section of a transmission line with linear materials (R, L, G, C, Z0 or S-parameters per length), use the built-in line solver first: see "Transmission lines" in `fh guide grid`. It needs no gmsh and no GetDP.

For the S-parameters of waveguides, transmission lines, steps, vias and filters, or the resonant modes of a cavity, in parts that are boxes or prisms of polygons and circles along 1 axis: use the built-in grid solver first (`fh guide grid`, section "Electromagnetics"). It needs no gmsh and no GetDP, and the person can also solve its `grid.toml` in the page.

For a DC current (the resistance, the current density, the Joule heat and its temperature) or an electrostatic field (the capacitance matrix) in the same shapes: use the built-in grid solver first (`fh guide grid`, section "DC conduction and electrostatics"). It needs no gmsh and no GetDP.

For another electromagnetic request, for example eddy currents in 3D, a magnetic field, or a curved or sloped shape, write a GetDP case folder and solve it with `fh run CASE`.

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
