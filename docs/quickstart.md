# Quickstart

The workflow goes from a STEP file to a mesh, a solver input and results.
`fh model` converts the mesh and the case TOML into the solver input.
`fh run` does all of these steps in one command and shows the results in the browser.

## Start

Run these 2 commands in a clone of this repository, on macOS or Linux:

```sh
scripts/install.sh
fh serve
```

### The install

`scripts/install.sh` builds `fh`, `fh-solve-ccx` and the browser viewer (release builds).
It copies `fh`, `fh-solve-ccx` and the folder `viewer` to `~/.local/bin`.
Use `--prefix DIR` for another folder.
It also uses uv to make a Python 3.12 environment with build123d 0.13.0 in `~/.local/share/femahoy/venv` (`$XDG_DATA_HOME/femahoy/venv` when you set `XDG_DATA_HOME`).
Use `--venv DIR` for another folder.
The launcher `fh-python` in the install folder runs the Python of that environment, for example `fh-python part.py`.
A second run builds again and updates the files and the environment.

The script finds cargo in `~/.cargo/bin` when cargo is not on `PATH`.
The file `rust-toolchain.toml` sets the Rust version, so rustup needs no default toolchain.
Before the build, the script checks these tools:

| Tool | Install command when it is missing |
|---|---|
| Rust 1.97.0 with rustfmt, clippy and the `wasm32-unknown-unknown` target | `rustup toolchain install 1.97.0 --profile minimal --component rustfmt,clippy --target wasm32-unknown-unknown` |
| `wasm-bindgen-cli` at the version of `Cargo.lock` | `cargo install wasm-bindgen-cli --version 0.2.129 --locked` |
| uv | `curl -LsSf https://astral.sh/uv/install.sh \| sh` |
| A C linker (macOS: the Xcode command line tools) | `xcode-select --install` |

If a tool is missing, the script shows all the missing tools with their install commands, and builds nothing.
The script installs no tool itself. uv downloads Python 3.12 and build123d.
It also tells you when gmsh is missing (`brew install gmsh`), because `fh run` needs gmsh for a STEP file.
It also tells you when CalculiX (`ccx`) is missing. Each solve needs `ccx`: see [the CalculiX adapter](ccx.md).
At the end, it shows the installed files and the next command.
If the folder is not on `PATH`, the next command has the full path, and the script shows the line to add to your shell start file.

### The chat

`fh serve` opens the page in the browser. Write a request in the chat of the page. The agent makes the part, meshes it and solves it, and the viewer of the page shows the results ([chat](chat.md)).

### A case folder

`fh run validation/cases/bar-pressure --size 8` solves a case of the repository.
The case is a steel bar of 100 × 20 × 10 mm. A pressure of 100 MPa pulls one end face.
The exact von Mises stress is 100 MPa in all of the bar ([derivation](../validation/cases/bar-pressure/derivation.md)).

1. `fh run` uses the daemon of the state folder. `STATE` is the state folder, `~/.local/state/femahoy` by default. If no daemon answers, it starts `fh serve --no-open` in the background.
2. It runs `fh mesh` (gmsh) and `fh model` in a work folder, `STATE/cases/bar-pressure-HASH`.
3. It submits the input to the daemon and shows the events of the run.
4. It shows the result folder and `summary.toon`, and opens the run in the browser.

Output excerpt (paths cut, `STATE` for the state folder):

```text
fh run: started a daemon for STATE (log STATE/daemon.log)
fh run: mesh: fh mesh validation/cases/bar-pressure/bar.step --case validation/cases/bar-pressure -o STATE/cases/bar-pressure-f069ef66a99460ee/bar.msh --size 8
nodes	844
elements	373
min_scaled_jacobian	0.1990
fh run: preview: the page shows bar-pressure (preview 1) with its supports and loads
fh run: run-000001 solves STATE/cases/bar-pressure-f069ef66a99460ee/bar.inp
  queued
  running
  succeeded
fh run: run-000001 succeeded in 0.19 s
fh run: results in STATE/runs/run-000001/out
fh run: summary.toon:
  ...
  status: ok
  ...
  solver: "ccx 2.23, unvalidated"
fh run: page http://127.0.0.1:53513/?token=TOKEN&run=run-000001
fh run: opened run-000001 in the browser
fh run: the daemon stays alive for the browser. Stop it with: fh serve --stop
```

The browser shows the bar with the contour of `MISES`.
The legend shows `MISES (MPa), per element` and one band, `constant 100.0`, because the stress is the same in all of the bar.
Drag to turn the bar, and scroll to zoom ([viewer](viewer.md)).

The daemon stays alive after `fh run`, so the page keeps the results, and the next `fh run` uses the same daemon.
Stop it with `fh serve --stop`.
Ctrl+C during a run cancels the run. A second Ctrl+C ends `fh run` at once.

### Your own model

`fh run` also takes these inputs:

| Command | What it solves |
|---|---|
| `fh run model.inp` | The `.inp` file as it is. |
| `fh run CASE_DIR` | A case folder with `case.toml` and one STEP file: `fh mesh`, `fh model`, then the solve. `CASE_DIR/case.toml` and `CASE_DIR/PART.step` do the same. |

`fh run` does not change the case folder. Its mesh and input go to the work folder.
`--size H` goes to `fh mesh`. `--no-open` (or `FH_NO_OPEN=1`) keeps the browser closed.
[`fh` commands](reference/fh.md#fh-run) gives the exit codes and the messages.

The sections below do the same steps one at a time.

## 1. Prepare the programs

Run these commands from the repository root in an interactive terminal.
`scripts/install.sh` (see [Start](#start)) puts `fh`, `fh-solve-ccx` and `fh-python` on `PATH`.
The solve step needs CalculiX (`ccx`, [the CalculiX adapter](ccx.md)).
The CAD step uses `fh-python`, the Python with build123d that the install makes.
The mesh step needs an external gmsh executable.

```sh
WORK=$(mktemp -d)
fh --help
```

The examples ran with build123d 0.13.0 and gmsh 4.15.2-git.
Output excerpts below omit temporary paths, progress and timings.

## 2. Make a STEP file and check its units

The [bar recipe](../scripts/cad/bar.py) makes a 100 × 20 × 10 mm box.
It exports a STEP file with a millimetre unit record.

```sh
fh-python scripts/cad/bar.py "$WORK/bar.step"
```

Use the existing [bar-tension case TOML](../validation/cases/bar-tension/case.toml) for the unit declaration.
Its first keys include these values.

```toml
schema = 3
id = "bar-tension"
units = "SI-mm"
```

This is an excerpt, not a complete case file.
`--case` needs the full case and its derivation file.
The [case schema](../crates/validation/SCHEMA.md) defines all required keys.

| System | Length | Force | Stress | Mass | Time |
|---|---|---|---|---|---|
| `SI-mm` | mm | N | MPa | tonne | s |
| `SI-m` | m | N | Pa | kg | s |

`fh mesh` (section 3) reads the unit record of the STEP file.
It refuses a STEP file with no unit record (`unit.step.missing`) or with other units than the run (`unit.step.mismatch`).

## 3. Mesh the STEP file with gmsh

Use the same unit system as the case TOML.
This `fh mesh` command takes `--units` and does not read the case TOML. Section 6 gives `fh mesh` the case with `--case`.
If gmsh is outside `PATH`, add `--gmsh /path/to/gmsh`.

```sh
fh mesh "$WORK/bar.step" -o "$WORK/bar.msh" --units SI-mm --size 5
```

The command returned exit code 0. Output excerpt:

```text
units	SI-mm
order	2
nodes	2038
elements	1015
min_scaled_jacobian	0.1971
type	C3D10	1015
```

`fh mesh` refuses a mesh with an inverted element (`mesh.jacobian.negative`).

## 4. Write the solver input and run the solver

`fh-solve-ccx` reads `.inp`, not `.msh` or case TOML.
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
The command keeps `C3D10` as the element. It refuses another element type.
`fh model` writes the line `** units: SI-mm` into the input. Section 6 shows named regions and a pressure load.

```sh
fh model "$WORK/bar.msh" --case validation/cases/bar-tension -o "$WORK/bar.inp"
```

A `cancel` line on stdin stops the solver. The end of stdin does not stop it (`docs/solver-protocol.md`).
You can run this command from a script or an agent shell.
Use a new result directory for each run.

```sh
fh-solve-ccx "$WORK/bar.inp" "$WORK/run-bar"
```

The bar has a prescribed end displacement of 0.1 mm. The exact von Mises stress is 210 MPa.
The solver prints only the line `fh-solver-protocol 2`. Section 5 reads the results.
The `ccx` files are in `$WORK/run-bar/ccx`.

## 5. Read the results

```sh
cat "$WORK/run-bar/summary.toon"
cat "$WORK/run-bar/results.pvd"
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
run_id: run-bar
solver: "ccx 2.23, unvalidated"
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
fh-solve-ccx "$WORK/press.inp" "$WORK/run-press"
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

The exact stress is 100 MPa. `fh validate` checks it (section 7).

`fh run validation/cases/bar-pressure --size 8` does these 3 steps in one command (see [Your own model](#your-own-model)).

`fh` refuses a region that selects no face, and a region that the mesh does not hold (`mesh.region.missing`).
The [bar-pressure-m case](../validation/cases/bar-pressure-m/case.toml) is the same model in m, N and Pa.

## 7. Run validation

Run this command from the repository root.
`fh validate` locates `fh-solve-ccx` beside `fh` or on `PATH`, or through `FH_SOLVE`.
If `ccx` does not start, `fh validate` stops with exit code 1 and tells how to install it.

```sh
fh validate
```

Output excerpt:

```text
bar-pressure    pass
bar-pressure-m  pass
bar-tension     pass
cantilever-c3d10        pass
...
warpage-thick-copper    pass
9 cases: 9 pass, 0 fail
```

The full output compares computed values with references, tolerances and margins.
These results apply to the supplied validation cases, not to a new model.

## What is not built yet

`fh` has no `results`, `plot`, `init`, `team` or `skills` command yet.
