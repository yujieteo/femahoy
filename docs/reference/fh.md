# `fh` commands, diagnostics, and exit codes

`fh` implements `mesh`, `validate`, `run`, and `serve`.

## Dispatch and help

| Invocation | Output | Exit |
|---|---|---:|
| `fh`, `fh -h`, `fh --help` | Usage and command list on stdout. | 0 |
| Unknown command or invalid top-level arguments | `fh: unknown argument: ARGUMENTS` and usage on stderr. | 2 |

A command error returned to the dispatcher prints `fh COMMAND: MESSAGE` on stderr and exits with 2.
This includes command argument errors. Subcommands do not implement `-h` or `--help`.
The command sources define more `mesh` options than the top-level help text shows.

## `fh mesh`

```text
fh mesh FILE.step --units SI-mm|SI-m [-o OUT.inp] [--size H]
  [--face NAME=SEL]... [--nodes NAME=SEL]... [--volume NAME=box:X0,Y0,Z0,X1,Y1,Z1]...
  [--refine NAME[,NAME]...=H]... [--shell]
```

Source: [mesh.rs](../../crates/cli/src/mesh.rs).

The command accepts `.step` and `.stp`, without regard to extension case.
It runs external gmsh for a 3D mesh of order 2 (`C3D10`) in `msh41` format, then writes the nodes, the elements and the named sets as a CalculiX input.
With `--shell` it runs gmsh for a 2D mesh of order 2 (`S6`, the 6-node shell triangle) of the surfaces of the file.
The input has no material, no step and no load. The chat includes it with `*INCLUDE` in the input that it writes.

| Option | Default or rule |
|---|---|
| `-o` | Input path with the extension changed to `.inp`. |
| `--size` | No value by default. Positive finite number, sent as gmsh `-clmax`. |
| `--units` | Required: `SI-mm` or `SI-m`. STEP units must match. The command writes `** units: SI-mm` (or `SI-m`) into the output. |
| `--face NAME=SEL` | Repeatable. A node set `NAME` and a surface `NAME` of the faces of the part that lie fully inside the selection, within 1e-6 of the diagonal of the part. With `--shell`, the part has edges, not faces. The surface is the edges of the shells: `S3` and up in the `*DSLOAD` face number, with edge `k` from corner `k` to corner `k+1`. |
| `--nodes NAME=SEL` | Repeatable. A node set of the nodes in the selection. |
| `--volume NAME=box:...` | Repeatable. An element set of the elements in the box. Each element must be in exactly 1 box. With `--shell`, a box selects surfaces. |
| `--refine NAME[,NAME]...=H` | Repeatable. Positive finite number H, below `--size`. Each `NAME` is the name of a `--face`. The elements at that set have the size H. The size grows to `--size` away from the set, by 0.3 for each unit of distance. Needs `--size`. Each name is used once. |
| `--shell` | Off by default. Mesh the surfaces of the STEP file as `S6` shells. The file must have no volume. The mid-surface is the work of the chat (build123d). |

`SEL` is a plane (`x:V`, `y:V` or `z:V`), a box (`box:X0,Y0,Z0,X1,Y1,Z1`) or, for `--nodes` only, a point (`point:X,Y,Z`).
A name uses letters, digits and `_`, and not `ALL` or `NALL`. Each name is used once.
`NALL` is the node set of all nodes. `ALL` is the element set of all elements.

`fh mesh` writes the gmsh script.
The script sets the STEP target unit and uses `SOLID` for one material without a `--volume` box.
For `--volume` boxes, it fragments the bodies and creates one physical volume group per box.
It checks that each solid element belongs to exactly one box.
For `--refine`, the script makes a gmsh `Distance` field and a `Threshold` field for each refined set (faces for a solid, curves with `--shell`). The `Min` of the fields is the background size field, and the sizes from points, from curvature and from the boundary are off.
With `--shell`, the script meshes in 2D, uses `SHELL` for one material without a `--volume` box, and fragments surfaces. `--face` selects curves.
The output has no `*SHELL SECTION`. The chat writes the thickness and the material.

`fh mesh` uses the program in a non-empty `FH_GMSH`, else `gmsh` on `PATH`, else `/Applications/Gmsh.app/Contents/MacOS/gmsh`.
The minimum gmsh version is 4.8.
The command replaces an existing output file only when the mesh succeeds.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Mesh success | stdout: tab-separated `ok`, `gmsh`, `units`, `nodes`, `elements`, `min_scaled_jacobian`, `type`, `nset`, `elset`, and `surface` (element-face count) rows. | 0 |
| gmsh warning or different release | stdout: `warning<TAB>MESSAGE`. gmsh lines that start with `Warning` are retained. | 0 |
| Missing option value, invalid number, selection, units, or name | stderr: `fh mesh: MESSAGE` | 2 |
| Unknown option, no input, or more than 1 input | Same prefix. Includes mesh usage. | 2 |
| Input read, extension, path, or temporary-file error | `fh mesh: MESSAGE` | 1 |
| No `--units` | `fh mesh: unit.missing: ...` | 1 |
| No STEP length record | `fh mesh: unit.step.missing: ...` | 1 |
| Multiple STEP length units | `fh mesh: unit.step.conflict: ...` | 1 |
| STEP unit differs from `--units` | `fh mesh: unit.step.mismatch: ...` | 1 |
| gmsh not found or cannot start | `fh mesh: gmsh is not installed. ...` or `cannot start gmsh (PATH): ERROR` | 1 |
| Unreadable or old version | `fh mesh: cannot read the gmsh version from ...` or `gmsh VERSION is too old, ...` | 1 |
| Child failure or a line that starts with `Error` | `fh mesh: gmsh failed (exit code N)` or `gmsh failed (stopped by a signal)`, with error text. | 1 |
| Missing output, unreadable mesh, or no volume elements | `fh mesh: gmsh wrote no mesh file: PATH`, `the mesh is not readable (PATH): ERROR`, or `gmsh wrote a mesh with no volume elements: PATH`. For a file with only surfaces, the last one says to use `--shell`. With `--shell`, it is `no surface elements`. | 1 |
| `--shell` with a file that has a volume | `fh mesh: mesh.shell.solid: ...` | 1 |
| An element with a corner Jacobian of 0 or less | `fh mesh: mesh.jacobian.negative: ...` with the count and example element IDs | 1 |
| A volume (a surface with `--shell`) with no physical group, in a file with physical groups | `fh mesh: mesh.volume.unnamed: ...` | 1 |
| A `--face` that selects no whole face of the part | `fh mesh: mesh.face.missing: ...` | 1 |
| A `--face` that is not on the outside of the solid | `fh mesh: mesh.face.inner: ...` | 1 |
| A `--nodes` selection with no node | `fh mesh: mesh.set.empty: ...` | 1 |
| A name used twice | `fh mesh: mesh.name.duplicate: ...` | 1 |
| A `--refine` name with no `--face`, with no `--size`, or with H not below `--size` | `fh mesh: --refine NAME: there is no --face NAME`, `it needs --size, ...` or `H must be below --size S` | 1 |
| A `--refine` with no `=H`, a bad number, or a name used twice | `fh mesh: --refine needs NAME=H` or `--refine names NAME twice` | 2 |
| A `--volume` box with no element, or an element in no box or in 2 boxes | `fh mesh: mesh.volume.empty: ...` or `mesh.volume.coverage: ...` | 1 |

The child exit code is diagnostic text. `fh mesh` returns 1 for a child failure.
Some operating-system errors supply their own message after the command prefix.

## `fh validate`

```text
fh validate [--root DIR]
```

Sources: [validate.rs](../../crates/cli/src/validate.rs),
[measure.rs](../../crates/cli/src/measure.rs), and [harness](../../crates/validation/src/run.rs).

`--root` defaults to `.`. The normal run loads case directories under `DIR/validation/cases`, sorted by directory name.
Each case folder holds a CalculiX input (`job.inp`) and a check file (`check.toml`). The case solves the input, then compares each quantity with its reference.
The solver is `ccx` (`FH_CCX`, else `ccx` on PATH). If `ccx` does not start, `fh validate` stops before the first case.
Each solve has a limit of 300 seconds. At the limit, the run stops `ccx` and the case cannot run.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Each case passes | stdout: `ID<TAB>pass`, quantity rows, and `N cases: P pass, F fail`. | 0 |
| `ccx` does not start | stderr: `fh validate: cannot start ccx (PATH): ERROR. fh needs CalculiX: install it as README.md tells, then put ccx on PATH or set FH_CCX` | 1 |
| Case failure | stdout: case row with `fail: REASON`, then summary. | 1 |
| The solve or the measurement cannot run | stdout: case row with `cannot run: REASON`. | 1 |
| Fixture load error | stderr: `fh validate: FILE[:LINE]: MESSAGE` | 1 |
| The input has an `*INCLUDE` file that is missing | stderr: `fh validate: PATH/job.inp: MESSAGE` | 1 |
| Missing option value | `fh validate: --root needs a directory` | 2 |
| Any other argument | `fh validate: unknown option: ARGUMENT` | 2 |
| Empty case directory | `fh validate: no cases under PATH` | 2 |

Quantity rows contain the case ID, quantity name, `computed`, `reference`, `tolerance` and `margin`.
Read or solver failures, absent result fields, empty sets, and invalid reductions can supply the failure reason.
See the [check file reference](check-toml.md) for the fields and the pass rules.

## `fh run`

```text
fh run MODEL.inp [--preview] [--timeout SECONDS] [--memory MIB]
```

Source: [run.rs](../../crates/cli/src/run.rs).

The command solves a model with `ccx` and prints the progress. It does not need `fh serve`. The page that `fh serve` opened reads the files of the run from the state folder, so it shows the model and the results.

`MODEL.inp` is a CalculiX input file (the extension in any case). `fh run` gives it to `ccx` as it is, with each file that it includes (`*INCLUDE`). The run solves a copy, so a later edit of the file does not change the run.
`ccx` is `$FH_CCX`, else `ccx` on `PATH`.

| Option | Default or rule |
|---|---|
| `--timeout SECONDS` | No limit by default. A number above 0. At the limit, `fh run` stops `ccx`. |
| `--memory MIB` | 85 percent of the RAM of the machine by default. A number above 0. At the limit, `fh run` stops `ccx`. |
| `--preview` | Write the preview for the page, then stop. No solve occurs. |

The state folder is `$FH_STATE`, the same default as `fh serve`. Each run has its own folder `runs/RUN` in it:

| Path | Content |
|---|---|
| `source` | The path of the input file. |
| `in/input.inp` | The read-only copy of the input, with the copies of its `*INCLUDE` files. |
| `out/` | The run folder of `ccx`: the `ccx` files in `ccx/`, `step-N.vtu` and `results.pvd`. |

`results.pvd` is the last file that the run writes. The page lists a run when this file exists.

Before the solve, `fh run` copies the input to `previews/N` in the state folder.
The page then shows the model with the faces of each support and each load, the direction of each load, and the names of the sets.
The line `fh run: preview: the page shows NAME (preview N) with its supports and loads` tells this. NAME is the input file.
With `--preview`, `fh run` prints `fh run: --preview: no solve` and stops.

When `fh run` ends for any reason, `ccx` stops. Ctrl+C, SIGTERM and SIGHUP stop `ccx` and end `fh run` with 130. If `fh run` gets SIGKILL, a guard process of the solver stops `ccx`. `Stop` in the chat sends SIGTERM to the process group of the agent, so `fh run` gets SIGTERM.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| `--preview`, and the preview is written | stdout: the preview line and `fh run: --preview: no solve` | 0 |
| The run succeeded | stdout: the preview line, `fh run: RUN solves FILE`, a line `  progress step S increment I` for each increment, `fh run: RUN succeeded in S s`, `results in DIR` and the paths of `job.dat` and `job.sta`. When the solve wrote no field, a line says that the page has no field to show. | 0 |
| `ccx` refused the input, the run hit a limit, or the run failed | stderr: `fh run: RUN failed: MESSAGE`. The message names the input file, not the copy in the state folder. | 1 |
| The preview or the run folder cannot be written | `fh run: the preview failed: MESSAGE` or `fh run: the run folder failed: MESSAGE` | 1 |
| No input, unknown option, bad value (for example `--timeout needs a number above 0`) | `fh run: MESSAGE` and the usage | 2 |
| The input does not exist | `fh run: PATH: ERROR (run.input.missing)` | 2 |
| The input is not an `.inp` file | `fh run: PATH: fh run takes a CalculiX input file (.inp) (run.input.kind)` | 2 |
| Ctrl+C, SIGTERM or SIGHUP during the solve | stderr: `fh run: RUN was cancelled` | 130 |

## `fh serve`

```text
fh serve
```

Source: [serve.rs](../../crates/daemon/src/serve.rs).

The command runs the daemon on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP.
`fh serve` takes no option. The state folder is `$FH_STATE`, else `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. `fh run` has the same default. The port is a free port.
The daemon reads the run and preview folders that `fh run` writes. It does not start `ccx`.
The page is the wasm viewer, from `FH_VIEWER` when set, otherwise the `viewer` folder beside `fh`.
The page has the chat with the agent.

After the start on macOS, `fh serve` opens its page in the browser with `open URL`. Launch Services gives the address to the default browser, so only the short-lived `open` process has the token in its command line.
On other systems, `fh serve` opens no browser. It only prints the address.
A value of `FH_NO_OPEN` that is not empty and not `0` stops the open, for tests and for a machine with no screen.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Start | stdout: `fh serve: http://127.0.0.1:PORT/?token=TOKEN`, the viewer folder (or a line that says the wasm viewer is not built), the state folder, and `fh serve: opened the page in the browser` (or `no browser (CAUSE)`). | |
| Stop signal | stdout: `fh serve: stopped`. | 0 |
| The daemon cannot start | stderr: `fh serve: DIR: MESSAGE (CODE)`, for example `daemon.running`. | 1 |
| Any other argument | `fh serve: unknown option: ARGUMENT` | 2 |
