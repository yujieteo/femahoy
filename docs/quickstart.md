# Quickstart

The workflow goes from a STEP file to a mesh, a solver input and results.
`fh model` converts the mesh and the case TOML into the solver input.

## 1. Prepare the programs

Run these commands from the repository root in an interactive terminal.
You need Rust and Cargo to build `fh` and `fh-solve`.
The CAD step needs external Python, uv and build123d.
The mesh step needs an external gmsh executable.

```sh
WORK=$(mktemp -d)
cargo build --bin fh --bin fh-solve --target-dir "$WORK/target"
export PATH="$WORK/target/debug:$PATH"
uv venv -p 3.12 "$WORK/venv"
uv pip install -p "$WORK/venv/bin/python" build123d
fh --help
```

The examples ran with build123d 0.13.0 and gmsh 4.15.2-git.
Output excerpts below omit temporary paths, progress and timings.

## 2. Make a STEP file and check its units

The [bar recipe](../scripts/cad/bar.py) makes a 100 × 20 × 10 mm box.
It exports a STEP file with a millimetre unit record.

```sh
"$WORK/venv/bin/python" scripts/cad/bar.py "$WORK/bar.step"
```

Use the existing [bar-tension case TOML](../validation/cases/bar-tension/case.toml) for the unit declaration.
Its first keys include these values.

```toml
schema = 2
id = "bar-tension"
units = "SI-mm"
class = "N"
```

This is an excerpt, not a complete case file.
`--case` needs the full case, its mesh files and its derivation file.
The [case schema](../crates/validation/SCHEMA.md) defines all required keys.

| System | Length | Force | Stress | Mass | Time |
|---|---|---|---|---|---|
| `SI-mm` | mm | N | MPa | tonne | s |
| `SI-m` | m | N | Pa | kg | s |

```sh
fh info "$WORK/bar.step" --case validation/cases/bar-tension
```

Output excerpt:

```text
kind: step
step_unit: mm
case_units: SI-mm
units_match: yes
```

## 3. Mesh the STEP file with gmsh

Use the same unit system as the case TOML.
This `fh mesh` command takes `--units` and does not read the case TOML. Section 6 gives `fh mesh` the case with `--case`.
If gmsh is outside `PATH`, add `--gmsh /path/to/gmsh`.

```sh
fh mesh "$WORK/bar.step" -o "$WORK/bar.msh" --units SI-mm --order 2 --size 5
fh info "$WORK/bar.msh" --case validation/cases/bar-tension
```

Both commands returned exit code 0. The info command produced this excerpt.

```text
nodes: 2038
elements: C3D10=1015
bbox_min: [0.0, 0.0, 0.0] mm
bbox_max: [100.0, 20.0, 10.0] mm
volume: 2.000000e4
min_scaled_jacobian: 0.1971
```

For `.msh` input, `--case` supplies unit labels. It does not verify a unit record in the mesh.

## 4. Write the solver input and run the solver

`fh-solve` reads `.inp`, not `.msh` or case TOML.
`fh model` writes the `.inp` file from the mesh and the `[model]` table of a case.
The [bar-tension case](../validation/cases/bar-tension/case.toml) has that table.
It holds the material, node sets on coordinate planes, boundary conditions and loads.
`fh mesh` records the mesh units in `bar.msh.units`.
`fh model` refuses a mesh with no record, or with other units than the case.

```toml
[[model.set]]
name = "TIP"
axis = "x"
at = 100.0

[[model.boundary]]
set = "TIP"
first_dof = 1
value = 0.1
```

A `[[model.cload]]` table applies a force to each node of a set. It is not a total force.
The command keeps `C3D10` as the element. It accepts hex8 only for an extruded part.
`fh model` writes the line `** units: SI-mm` into the input. Section 6 shows named regions and a pressure load.

```sh
fh model "$WORK/bar.msh" --case validation/cases/bar-tension -o "$WORK/bar.inp"
```

Keep stdin open. End of stdin or a `cancel` line stops the solver.
Run this command in the interactive terminal, without a pipe or input redirection.
Use a new result directory for each run.

```sh
fh-solve "$WORK/bar.inp" "$WORK/run-bar" --formats text,vtu
```

The bar has a prescribed end displacement of 0.1 mm. The exact von Mises stress is 210 MPa.
Output excerpt (the `max_u` value includes the Poisson contraction at the corner):

```text
fh-solver-protocol 2
step 1 max_u=1.0022474744293448e-1 max_mises=2.1000000000001194e2 max_rf=1.881077767473042e3 free_dofs=5318 method=cholesky iterations=0
```

## 5. Read the results

```sh
cat "$WORK/run-bar/summary.toon"
cat "$WORK/run-bar/results.pvd"
head -n 9 "$WORK/run-bar/step-1.txt"
head -n 25 "$WORK/run-bar/step-1.vtu"
```

The TOON summary contains this excerpt.

```text
schema: fh.run
version: 1.1.0
source: bar.inp
units: SI-mm
status: ok
precision: 4
solver: core
results: results.pvd
```

TOON rounds numbers to 4 significant figures. The VTU file retains full precision.
The PVD file states the units and names the VTU result.

```xml
<Collection units="SI-mm">
<DataSet timestep="1" part="0" file="step-1.vtu"/>
```

The VTU file contains point fields `U` and `RF`, and cell fields `S` and `MISES`.
The last command reads the XML header before the binary result arrays.
`U` is displacement. `RF` is reaction force. `S` is stress. `MISES` is von Mises stress.
The stress components are `S11 S22 S33 S12 S23 S13`.
The solver reads the units from the `** units:` line of the input. It uses them only as a label.
An input with no such line gives `units: unspecified`.
Interpret the numbers in the declared unit system: mm, N and MPa for `SI-mm`.

## 6. Name regions and apply a pressure

The [bar-pressure case](../validation/cases/bar-pressure/case.toml) names 4 faces of the bar as regions.
A region selects the faces of the part on a plane, or the faces inside a box.
A `[[model.dsload]]` table applies a pressure to a region. A negative pressure pulls the face.

```toml
[[model.region]]
name = "LOAD"
axis = "x"
at = 100.0

[[model.dsload]]
surface = "LOAD"
pressure = -100.0
```

Give `fh mesh` the case with `--case`, so that it makes a gmsh physical surface for each region.

```sh
fh mesh "$WORK/bar.step" --case validation/cases/bar-pressure --size 8 -o "$WORK/press.msh"
fh model "$WORK/press.msh" --case validation/cases/bar-pressure -o "$WORK/press.inp"
fh-solve "$WORK/press.inp" "$WORK/run-press" --formats text,vtu
```

Output excerpt of `fh mesh` (element-face counts of each region):

```text
elset	SOLID	373
surface	SUPPORT	14
surface	LOAD	14
surface	SYM_Y	54
surface	SYM_Z	88
```

`fh model` writes each region as `*NSET` and `*SURFACE, TYPE=ELEMENT`, and the pressure as `*DSLOAD`:

```text
*DSLOAD
LOAD, P, -100.0
```

The exact stress is 100 MPa. Output excerpt of `fh-solve`:

```text
step 1 max_u=4.7726070210919874e-2 max_mises=1.0000000000000375e2 max_rf=1.0185185185184826e3 free_dofs=2145 method=cholesky iterations=0
```

`fh` refuses a region that selects no face, and a region that the mesh does not hold (`mesh.region.missing`).
The [bar-pressure-m case](../validation/cases/bar-pressure-m/case.toml) is the same model in m, N and Pa.

## 7. Run validation

Run this command from the repository root.
`fh validate` locates `fh-solve` beside `fh`, or through `FH_SOLVE`.

```sh
fh validate
```

Output excerpt:

```text
bar-pressure    N       pass
bar-pressure-m  N       pass
bar-tension     N       pass
cantilever      L       pass
...
patch-test-c3d8i        N       pass
16 cases: 16 pass, 0 fail
```

The full output compares computed values with references, tolerances and margins.
These results apply to the supplied validation cases, not to a new model.

## What is not built yet

`fh --help` lists `results`, `plot`, `init`, `team` and `skills` as not implemented.
There is no `fh run` command. Run `fh-solve` and read its result files directly.
