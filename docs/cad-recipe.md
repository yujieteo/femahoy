# CAD to mesh recipe (M2b, first part)

This recipe makes a part with build123d, meshes it with gmsh, and checks the units and the mesh with `fh info`.
It follows D-17, D-18 and D-19 in `DESIGN.md`. All tools except `fh` are separate programs that the user installs.
Tested with Python 3.12, build123d 0.13.0 and gmsh 4.15.2 on macOS arm64.

## Steps

1. Declare the units in `case.toml`: `units = "SI-mm"` or `units = "SI-m"`. There is no default.
2. Write the part with build123d. `scripts/cad/bar.py` is an example. It writes a STEP file in mm.
3. Check the STEP file: `fh info part.step --case <case folder>`.
   `fh` refuses with `unit.step.missing`, `unit.step.conflict` or `unit.step.mismatch`.
4. Copy `scripts/cad/bar.geo.in` and replace `@STEP@`, `@UNIT@`, `@SIZE@` and `@OUT@`.
   Use the `gmsh_target_unit` value that step 3 prints for `@UNIT@`.
5. Run `gmsh part.geo -0 -nopopup`. gmsh exits with code 0 when it makes no mesh, so do step 6 always.
6. Check the mesh: `fh info part.msh --case <case folder>`.
   `fh` refuses with `mesh.empty` or `mesh.jacobian.negative`.

## Rules

- The mesh is C3D10 (D-19). Use hex8 only for a part that is a 2D shape extruded along one axis.
  gmsh automatic hex meshing gave inverted elements on parts with fillets and holes.
- The `.geo` file sets `Geometry.OCCTargetUnit` in every run. gmsh keeps this option as state, and `.msh` has no unit.
- A part with more than 1 body needs `Coherence;` (the template has it). Without it the bodies share no nodes.
- STEP keeps no face names. The template selects faces by plane with `Surface In BoundingBox`.
- `fh info` reads the STEP unit record as text. It does not read the geometry.

## Tests

`cargo test -p fh-cli --test info` runs the recipe with gmsh and skips this test when gmsh is not on the `PATH`.
Set `FH_GMSH` to use another gmsh program. The test makes its STEP file with gmsh, so it needs no Python.
