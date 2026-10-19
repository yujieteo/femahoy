# Quickstart

The workflow goes from a STEP file to a mesh, a CalculiX input and results.
`fh mesh` writes the mesh and the named sets. You write the rest of the CalculiX input.
`fh run` solves the input and shows the results in the browser.

## Start

Run these 2 commands in a clone of this repository, on macOS or Linux:

```sh
scripts/install.sh
fh serve
```

### The install

`scripts/install.sh` builds `fh` and the browser viewer (release builds).
It copies `fh` and the folder `viewer` to `~/.local/bin`.
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
It also tells you when gmsh is missing (`brew install gmsh`), because `fh mesh` needs gmsh.
It also tells you when CalculiX (`ccx`) is missing. Each solve needs `ccx`: see [the CalculiX adapter](ccx.md).
At the end, it shows the installed files and the next command.
If the folder is not on `PATH`, the next command has the full path, and the script shows the line to add to your shell start file.

### The chat

`fh serve` opens the page in the browser. Write a request in the chat of the page. The agent makes the part, meshes it and solves it, and the viewer of the page shows the results ([chat](chat.md)).

### Your own input

`fh run job.inp` solves a CalculiX input file. CalculiX reads the file as it is, with each file that it includes (`*INCLUDE`).

1. `fh run` uses the daemon of the state folder. `STATE` is the state folder, `~/.local/state/femahoy` by default. If no daemon answers, it starts `fh serve --no-open` in the background.
2. It shows the supports and the loads of the input in the browser (`--preview` stops here).
3. It submits the input to the daemon and shows the events of the run.
4. It shows the result folder and the paths of the text results, and opens the run in the browser.

`--timeout SECONDS` and `--memory MIB` set the limits of the run. `--no-open` (or `FH_NO_OPEN=1`) keeps the browser closed.
The daemon stays alive after `fh run`, so the page keeps the results, and the next `fh run` uses the same daemon.
Stop it with `fh serve --stop`.
Ctrl+C during a run cancels the run. A second Ctrl+C ends `fh run` at once.
[`fh` commands](reference/fh.md#fh-run) gives the exit codes and the messages.

The sections below do the steps one at a time.

## 1. Prepare the programs

Run these commands from the repository root in an interactive terminal.
`scripts/install.sh` (see [Start](#start)) puts `fh` and `fh-python` on `PATH`.
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

You tell `fh mesh` the unit system with `--units`. There is no default.

| System | Length | Force | Stress | Mass | Time |
|---|---|---|---|---|---|
| `SI-mm` | mm | N | MPa | tonne | s |
| `SI-m` | m | N | Pa | kg | s |

`fh mesh` (section 3) reads the unit record of the STEP file.
It refuses a STEP file with no unit record (`unit.step.missing`) or with other units than `--units` (`unit.step.mismatch`).

## 3. Mesh the STEP file with gmsh

`fh mesh` writes the nodes, the `C3D10` elements and the sets that you name. The sets are selections on the part:
a plane (`x:V`, `y:V`, `z:V`), a box (`box:X0,Y0,Z0,X1,Y1,Z1`) or, for `--nodes`, a point (`point:X,Y,Z`).
If gmsh is outside `PATH`, add `--gmsh /path/to/gmsh`.

```sh
fh mesh "$WORK/bar.step" --units SI-mm --size 5 -o "$WORK/mesh.inp" \
  --face FIXED=x:0 --face LOAD=x:100 --face YSYM=y:0 --face ZSYM=z:0 --nodes TIP=point:100,0,0
```

The command returned exit code 0. Output excerpt:

```text
units	SI-mm
nodes	2038
elements	1015
min_scaled_jacobian	0.1971
nset	NALL	2038
elset	ALL	1015
nset	FIXED	57
nset	LOAD	57
nset	YSYM	217
nset	ZSYM	465
surface	FIXED	22
surface	LOAD	22
surface	YSYM	86
surface	ZSYM	208
nset	TIP	1
```

Each `--face` makes a node set and an element surface of the same name. `NALL` is the set of all nodes. `ALL` is the set of all elements.
The first lines of `mesh.inp` are comments, and the second comment is `** units: SI-mm`.
`fh mesh` refuses a mesh with an inverted element (`mesh.jacobian.negative`), a set that selects nothing (`mesh.set.empty`) and a name that it uses twice (`mesh.name.duplicate`).

## 4. Write the CalculiX input and run the solver

Write `job.inp` next to `mesh.inp`. It includes the mesh and holds the material, the section, the step, the supports, the loads and the output requests.
Use any CalculiX keyword ([the CalculiX adapter](ccx.md)).
This input pulls the bar with a pressure of 100 MPa on the face `LOAD`. A negative pressure in `*DSLOAD` pulls the face, and a positive pressure pushes into the body.
The 3 symmetry faces and the face `FIXED` keep the bar from moving as a rigid body.

```text
*INCLUDE, INPUT=mesh.inp
*MATERIAL, NAME=STEEL
*ELASTIC
210000.0, 0.3
*SOLID SECTION, ELSET=ALL, MATERIAL=STEEL
*STEP, NAME=PULL
*STATIC
*BOUNDARY
FIXED, 1, 1
YSYM, 2, 2
ZSYM, 3, 3
*DSLOAD
LOAD, P, -100.0
*NODE PRINT, NSET=TIP
U
*NODE PRINT, NSET=FIXED, TOTALS=YES
RF
*NODE FILE
U, RF
*EL FILE
S
*END STEP
```

`*NODE PRINT` and `*EL PRINT` write text values to `job.dat`. `*NODE FILE` and `*EL FILE` write the fields that the page shows.

`fh run` shows the supports and the loads in the page, gives the input to the daemon, which starts `ccx`, and shows the results. Ctrl+C stops the solve.
You can run this command from a script or an agent shell.

```sh
cd "$WORK" && fh run job.inp --no-open
```

Output excerpt (paths cut, `STATE` for the state folder):

```text
fh run: preview: the page shows job.inp (preview 1) with its supports and loads
fh run: run-000001 solves job.inp
  queued
  running
  succeeded
fh run: run-000001 succeeded in 0.28 s
fh run: results in STATE/runs/run-000001/out
fh run: text results STATE/runs/run-000001/out/ccx/job.dat (print requests), STATE/runs/run-000001/out/ccx/job.sta (steps)
fh run: page http://127.0.0.1:PORT/?token=TOKEN&run=run-000001
```

Set `OUT` to the results folder. Section 5 reads the results. The `ccx` files are in `$OUT/ccx`.

## 5. Read the results

The numbers that you asked for are in the text files of `ccx`:

```sh
grep -A2 displacements "$OUT/ccx/job.dat"
cat "$OUT/ccx/job.sta"
```

```text
 displacements (vx,vy,vz) for set TIP and time  0.1000000E+01

         5  4.761905E-02  0.000000E+00  0.000000E+00
```

The tip displacement is 100 MPa × 100 mm / 210000 MPa = 0.047619 mm, as the hand calculation gives.
The line `total force` in `job.dat` gives the reaction of the face `FIXED`: −2.000000E+04 N, which is 100 MPa × 200 mm².
`job.sta` has 1 line for each increment of each step.

The page reads the result file of `ccx` (`job.frd`). The run also writes it as VTU files:

```sh
cat "$OUT/results.pvd"
```

```xml
<Collection units="SI-mm">
<DataSet timestep="1" part="0" file="step-1.vtu"/>
```

The PVD file states the units and names a VTU file for each frame of the result. The VTU files keep full precision.
Each field of the `.frd` file is in the VTU file: here `U`, `RF` and `S`, and `MISES` (the von Mises stress that `fh` adds) and `ERROR` (that `ccx` writes).
`U` is displacement. `RF` is reaction force. `S` is stress, with the components `S11 S22 S33 S12 S23 S13`.
A step with no `*NODE FILE` or `*EL FILE` request has no field to show.
The units come from the `** units:` line of the input, or of a file that it includes. They are only a label.
An input with no such line gives `units="unspecified"` in the PVD file.
Interpret the numbers in the declared unit system: mm, N and MPa for `SI-mm`.

## 6. Use more of CalculiX

`job.inp` is a CalculiX input, so it can use any keyword that `ccx` knows: for example `*PLASTIC`, `*CONTACT PAIR`, `*FREQUENCY` or `*HEAT TRANSFER`, and the element types `S8R` or `B32`.
For a part with several materials, `--volume NAME=box:...` makes an element set for each material.
`fh run job.inp --preview` shows the supports and the loads and does not solve. Use it to check a model before a long solve.

The page shows each frame that `ccx` writes. A frequency step gives 1 frame for each mode, and `job.dat` has the frequencies.
The page draws a shell or a beam element as the solid element that `ccx` writes to the result file.

## 7. Run validation

Run this command from the repository root.
Each case in `validation/cases` is a CalculiX input (`job.inp`) and a small file `check.toml` with the reference values, the measures and the tolerances ([check file](reference/check-toml.md)).
`fh validate` solves each case with `ccx` (`FH_CCX`, else `ccx` on `PATH`).
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
