# CAD to mesh recipe (M2b, first part)

This recipe makes a part with build123d and meshes it with `fh mesh` (gmsh). `fh mesh` checks the units and the mesh.
It follows D-17, D-18 and D-19 in `DESIGN.md`. All tools except `fh` are separate programs. `scripts/install.sh` makes a Python environment with build123d, and its launcher `fh-python` runs that Python. The user installs gmsh.
Tested with Python 3.12.13, build123d 0.13.0 and gmsh 4.15.2 on macOS 27.0.1 arm64 (Apple M5).

## Steps

1. Declare the units in `case.toml`: `units = "SI-mm"` or `units = "SI-m"`. There is no default.
2. Write the part with build123d and run it with `fh-python part.py`. `scripts/cad/bar.py` is an example. It writes a STEP file in mm, or in m when the second argument is `m`.
3. Mesh the part: `fh mesh part.step --case <case folder> --size H -o part.msh`.
   `fh` refuses the STEP file with `unit.step.missing`, `unit.step.conflict` or `unit.step.mismatch`, and the mesh with `mesh.jacobian.negative`.

## Named regions

Do these steps when the case names support and load faces.

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

- The mesh is C3D10 (D-19). C3D10 is the only element.
- `fh mesh` sets `Geometry.OCCTargetUnit` in every run. gmsh keeps this option as state, and `.msh` has no unit.
- A part with more than 1 body needs a material box for each body. `fh mesh` then fragments the bodies, so they share nodes.
- STEP keeps no face names. `fh mesh` selects faces by plane or by box with `Surface In BoundingBox`.
- `fh mesh` reads the STEP unit record as text. It does not read the geometry.

## Tests

`cargo test -p fh-cli --test mesh --test model` runs `fh mesh` and `fh model` with gmsh, and skips those tests when gmsh is not found.
Set `FH_GMSH` to use another gmsh program. The tests use STEP files from gmsh or from the repository, so they need no Python.
