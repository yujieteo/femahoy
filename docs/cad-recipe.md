# CAD to mesh recipe (M2b, first part)

This recipe makes a part with build123d and meshes it with `fh mesh` (gmsh). `fh mesh` checks the units and the mesh.
It follows D-17, D-18 and D-19 in `DESIGN.md`. All tools except `fh` are separate programs. `scripts/install.sh` makes a Python environment with build123d, and its launcher `fh-python` runs that Python. The user installs gmsh.
Tested with Python 3.12.13, build123d 0.13.0 and gmsh 4.15.2 on macOS 27.0.1 arm64 (Apple M5).

## Steps

1. Write the part with build123d and run it with `fh-python part.py`. `scripts/cad/bar.py` is an example. It writes a STEP file in mm, or in m when the second argument is `m`.
2. Mesh the part: `fh mesh part.step --units SI-mm --size H -o mesh.inp`. The units are required. There is no default.
   `fh` refuses the STEP file with `unit.step.missing`, `unit.step.conflict` or `unit.step.mismatch`, and the mesh with `mesh.jacobian.negative`.
   `mesh.inp` holds the nodes (`NALL`), the `C3D10` elements (`ALL`) and the line `** units: SI-mm`.

## Named sets

Do these steps when the model needs support faces, load faces or several materials.

1. Name each set on the command line, by plane (`x:V`, `y:V`, `z:V`) or by box (`box:X0,Y0,Z0,X1,Y1,Z1`):
   `fh mesh part.step --units SI-mm --size H --face FIXED=x:0 --face TOP=z:10 --nodes TIP=point:100,5,5 -o mesh.inp`.
   `--face` makes a node set and a surface of the same name. `--nodes` makes a node set. `--volume NAME=box:...` makes an element set.
   `fh` refuses a set that selects nothing (`mesh.set.empty`), a face that is not on the outside of the solid (`mesh.face.inner`) and a volume with no physical group (`mesh.volume.unnamed`).
2. Write `job.inp` with `*INCLUDE, INPUT=mesh.inp`, the material, the section, the step, the supports and the loads. The sets of step 1 are the names in `*BOUNDARY`, `*CLOAD`, `*DSLOAD` and `*SOLID SECTION`.
3. Solve with `fh run job.inp`.

Each new mesh selects the faces again, so a change of the mesh size keeps the sets.
The case `validation/cases/bar-pressure` (mm) is an example of the sets and the loads in an input. `validation/cases/bar-pressure-m` uses m.

## Rules

- `fh mesh` writes C3D10 (D-19). The input of the chat can use any other element type that CalculiX has, with a mesh from another source.
- `fh mesh` sets `Geometry.OCCTargetUnit` in every run. gmsh keeps this option as state, and `.msh` has no unit.
- A part with more than 1 body needs a material box for each body. `fh mesh` then fragments the bodies, so they share nodes.
- STEP keeps no face names. `fh mesh` selects faces by plane or by box with `Surface In BoundingBox`.
- `fh mesh` reads the STEP unit record as text. It does not read the geometry.

## Tests

`cargo test -p fh-cli --test mesh` runs `fh mesh` with gmsh, and skips those tests when gmsh is not found.
Set `FH_GMSH` to use another gmsh program. The tests use STEP files from gmsh or from the repository, so they need no Python.
