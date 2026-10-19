# bar-tension derivation

A bar of 100 x 20 x 10 mm (`bar.step`) has E = 210000 MPa and nu = 0.3 (units SI-mm).
The face x = 0 has ux = 0. The face x = 100 has ux = 0.1 mm.
The faces y = 0 and z = 0 have uy = 0 and uz = 0. Thus the bar has no rigid motion.

The field is ux = 1e-3 x, uy = -nu 1e-3 y, uz = -nu 1e-3 z. It satisfies equilibrium and all boundary conditions.
It is linear, so C3D10 (quadratic) reproduces it exactly.

- Strain exx = 0.1 / 100 = 1e-3.
- Stress sxx = E exx = 210 MPa. The other stress components are 0.
- Area = 20 x 10 = 200 mm2. The reaction on x = 0 is -210 x 200 = -42000 N.

`bar.step` came from gmsh (OpenCASCADE `Box`), because build123d was not available. Its unit record is millimetre.
`scripts/cad/bar.py` makes the same box. Run `fh mesh` again to remake the mesh in `job.inp`.
