# Quickstart

The current workflow checks a STEP mesh and solves a separate `.inp` model.
The CLI does not yet convert that mesh into a complete solver input.

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

Use the existing [patch-test case TOML](../validation/cases/patch-test/case.toml) for the unit declaration.
Its first keys include these values.

```toml
schema = 2
id = "patch-test"
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
fh info "$WORK/bar.step" --case validation/cases/patch-test
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
`fh mesh` takes `--units` directly. It does not read the case TOML.
If gmsh is outside `PATH`, add `--gmsh /path/to/gmsh`.

```sh
fh mesh "$WORK/bar.step" -o "$WORK/bar.msh" --units SI-mm --order 2 --size 5
fh info "$WORK/bar.msh" --case validation/cases/patch-test
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

## 4. Run the solver on the supplied model

`fh-solve` reads `.inp`, not `.msh` or case TOML.
The [patch-test model](../fixtures/inp/patch-test.inp) supplies the material, section, boundary conditions and step.
It is separate from the bar mesh above.
Use a new result directory for each run.

Keep stdin open. End of stdin or a `cancel` line stops the solver.
Run this command in the interactive terminal, without a pipe or input redirection.

```sh
fh-solve fixtures/inp/patch-test.inp "$WORK/run-patch" --formats text,vtu
```

The command returned exit code 0. Output excerpt:

```text
fh-solver-protocol 2
step 1 max_u=1.7291616465790582e-3 max_mises=1.295888884125678e2 max_rf=1.4132547541048046e2 free_dofs=24 method=cholesky iterations=0
```

## 5. Read the results

```sh
cat "$WORK/run-patch/summary.toon"
cat "$WORK/run-patch/results.pvd"
head -n 9 "$WORK/run-patch/step-1.txt"
head -n 25 "$WORK/run-patch/step-1.vtu"
```

The TOON summary contains this excerpt.

```text
schema: fh.run
version: 1.1.0
source: patch-test.inp
units: unspecified
status: ok
precision: 4
solver: core
results: results.pvd
```

TOON rounds numbers to 4 significant figures. The VTU file retains full precision.
The PVD file names the VTU result.

```xml
<DataSet timestep="1" part="0" file="step-1.vtu"/>
```

The VTU file contains point fields `U` and `RF`, and cell fields `S` and `MISES`.
The last command reads the XML header before the binary result arrays.
`U` is displacement. `RF` is reaction force. `S` is stress. `MISES` is von Mises stress.
The stress components are `S11 S22 S33 S12 S23 S13`.
The solver does not read the case units. Its TOON and PVD files report `unspecified`.
Interpret the numbers in the consistent unit system of the input model.

## 6. Run validation

Run this command from the repository root.
`fh validate` locates `fh-solve` beside `fh`, or through `FH_SOLVE`.

```sh
fh validate
```

Output excerpt:

```text
cantilever      L       pass
lame-cylinder   L       pass
patch-test      N       pass
3 cases: 3 pass, 0 fail
```

The full output compares computed values with references, tolerances and margins.
These results apply to the supplied validation cases, not to the bar mesh or a new model.

## What is not built yet

`fh --help` lists `results`, `plot`, `serve`, `init`, `team` and `skills` as not implemented.
There is no `fh run` command.
The CLI does not yet create solver input from STEP geometry and case metadata.
Use `fh-solve` with a complete `.inp` model and read its result files directly.
