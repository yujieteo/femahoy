# CAD to mesh recipe (M2b, first part)

This recipe makes a part with build123d, meshes it with gmsh, and checks the units and the mesh with `fh info`.
It follows D-17, D-18 and D-19 in `DESIGN.md`. All tools except `fh` are separate programs that the user installs.
Tested with Python 3.12.13, build123d 0.13.0 and gmsh 4.15.2 on macOS 27.0.1 arm64 (Apple M5).

## Steps

1. Declare the units in `case.toml`: `units = "SI-mm"` or `units = "SI-m"`. There is no default.
2. Write the part with build123d. `scripts/cad/bar.py` is an example. It writes a STEP file in mm, or in m when the second argument is `m`.
3. Check the STEP file: `fh info part.step --case <case folder>`.
   `fh` refuses with `unit.step.missing`, `unit.step.conflict` or `unit.step.mismatch`.
4. Copy `scripts/cad/bar.geo.in` and replace `@STEP@`, `@UNIT@`, `@SIZE@` and `@OUT@`.
   Use the `gmsh_target_unit` value that step 3 prints for `@UNIT@`.
5. Run `gmsh part.geo -0 -nopopup`. gmsh exits with code 0 when it makes no mesh, so do step 6 always.
6. Check the mesh: `fh info part.msh --case <case folder>`.
   `fh` refuses with `mesh.empty` or `mesh.jacobian.negative`.

## Named regions

Use `fh mesh` in place of steps 4 and 5 when the case names support and load faces.

1. Declare each region in `case.toml` as a `[[model.region]]` table, by plane (`axis`, `at`) or by `box`. `crates/validation/SCHEMA.md` has the fields.
2. Mesh the part: `fh mesh part.step --case <case folder> --size H -o part.msh`.
   `fh mesh` puts every volume in the physical group `SOLID` and makes a gmsh physical surface for each region.
   `fh` refuses a region that selects no face (`mesh.region.missing`) and a region that is not on the outside of the solid (`mesh.region.inner`).
3. Write the solver input: `fh model part.msh --case <case folder> -o part.inp`.
   `fh model` writes each region as `*NSET` and `*SURFACE, TYPE=ELEMENT`, and each `[[model.dsload]]` as `*DSLOAD`.
   `fh` refuses a region that the mesh does not hold (`mesh.region.missing`) and a volume with no physical group (`mesh.volume.unnamed`).

Each new mesh selects the region faces again from the case, so a change of the mesh size keeps the regions.
The cases `validation/cases/bar-pressure` (mm) and `validation/cases/bar-pressure-m` (m) are examples.

## Rules

- The mesh is C3D10 (D-19). Use hex8 only for a part that is a 2D shape extruded along one axis.
  gmsh automatic hex meshing gave inverted elements on parts with fillets and holes.
- The `.geo` file sets `Geometry.OCCTargetUnit` in every run. gmsh keeps this option as state, and `.msh` has no unit.
- A part with more than 1 body needs `Coherence;` (the template has it). Without it the bodies share no nodes.
- STEP keeps no face names. The template and `fh mesh` select faces by plane or by box with `Surface In BoundingBox`.
- `fh info` reads the STEP unit record as text. It does not read the geometry.

## Tests

`cargo test -p fh-cli --test info` runs the recipe with gmsh and skips this test when gmsh is not on the `PATH`.
`cargo test -p fh-cli --test mesh --test model` runs `fh mesh` and `fh model` with gmsh, and skips those tests when gmsh is not found.
Set `FH_GMSH` to use another gmsh program. The tests use STEP files from gmsh or from the repository, so they need no Python.
`scripts/check-workflow.sh` runs the full path with build123d and gmsh (`docs/workflow-lock.md`).
