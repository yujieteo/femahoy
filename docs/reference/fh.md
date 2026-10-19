# `fh` commands, diagnostics, and exit codes

`fh` implements `mesh`, `model`, `validate`, `run`, and `serve`.
This reference comes from the [command dispatcher](../../crates/cli/src/main.rs) and the command sources below.
The help text was checked with `cargo +stable run -q -p fh-cli --bin fh -- --help`.

## Dispatch and help

| Invocation | Output | Exit |
|---|---|---:|
| `fh`, `fh -h`, `fh --help` | Usage and command list on stdout. | 0 |
| Unknown command or invalid top-level arguments | `fh: unknown argument: ARGUMENTS` and usage on stderr. | 2 |

A command error returned to the dispatcher prints `fh COMMAND: MESSAGE` on stderr and exits with 2.
This includes command argument errors. Subcommands do not implement `-h` or `--help`.
The help text mentions TOON precision, but these command handlers print the formats listed below.
The command sources define more `mesh` options than the top-level help text shows.

## `fh mesh`

```text
fh mesh FILE.(geo|step) (--case DIR | --units SI-mm|SI-m) [-o OUT.msh]
  [--size H] [--scale S] [--set NAME=VALUE]... [--timeout SECONDS] [--gmsh PATH]
```

Source: [mesh.rs](../../crates/cli/src/mesh.rs).

The command accepts `.geo`, `.step`, and `.stp`, without regard to extension case.
It runs external gmsh for a 3D mesh of order 2 (`C3D10`) in `msh41` format, then reads the result.

| Option | Default or rule |
|---|---|
| `-o` | Input path with the extension changed to `.msh`. |
| `--size` | No value by default. Positive finite number, sent as gmsh `-clmax`. |
| `--scale` | No value by default. Positive finite number, sent as gmsh `-clscale`. |
| `--set` | Repeatable. Name uses ASCII letters, digits, `_`, or `.`. Value is finite. Sent as `-setnumber`. |
| `--units` | `SI-mm`. Also accepts `SI-m`. STEP units must match. |
| `--timeout` | No limit by default. Positive finite duration. The version probe has a separate 30-second limit. |
| `--gmsh` | Overrides discovery. Otherwise use non-empty `FH_GMSH`, then a file on `PATH`, then the macOS app path. |

For a STEP file, `fh mesh` writes the gmsh script (the fixed script contract of M2b).
The script sets the STEP target unit and uses `SOLID` for one material without a box.
For material boxes, it fragments the bodies and creates one physical volume group per material.
It checks that each solid element belongs to exactly one material group,
and makes a physical surface for each `[[model.region]]` of the case.
A region holds the faces of the part that lie fully inside its plane or box, within 1e-6 of the diagonal of the part.

The macOS fallback is `/Applications/Gmsh.app/Contents/MacOS/gmsh`.
The minimum gmsh version is 4.8. A release outside 4.15 produces a warning.
The command removes an existing output file before the mesh run.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Mesh success | stdout: tab-separated `ok`, `gmsh`, `units`, `order`, `nodes`, `elements`, `min_scaled_jacobian`, `type`, `nset`, `elset`, and `surface` (element-face count) rows. | 0 |
| gmsh warning or different release | stdout: `warning<TAB>MESSAGE`. gmsh lines that start with `Warning` are retained. | 0 |
| Missing option value, invalid number, units, or set name | stderr: `fh mesh: MESSAGE` | 2 |
| Unknown option, no input, or more than 1 input | Same prefix. Includes mesh usage. | 2 |
| Cannot create the temporary run directory | `fh mesh: cannot make PATH: ERROR` | 2 |
| Input read, extension, path, or temporary-file error | `fh mesh: MESSAGE` | 1 |
| Refused `.geo` command | Same prefix. Refuses `SystemCall`, `NonBlockingSystemCall`, `OnelabRun`, `Include`, and non-`.msh` or non-literal `Merge`. | 1 |
| No STEP length record | `fh mesh: unit.step.missing: ...` | 1 |
| Multiple STEP length units | `fh mesh: unit.step.conflict: ...` | 1 |
| STEP unit differs from declared units | `fh mesh: unit.step.mismatch: ...` | 1 |
| gmsh not found or cannot start | `fh mesh: gmsh is not installed. ...` or `cannot start gmsh (PATH): ERROR` | 1 |
| Unreadable or old version | `fh mesh: cannot read the gmsh version from ...` or `gmsh VERSION is too old, ...` | 1 |
| Cannot replace output | `fh mesh: cannot replace PATH: ERROR` | 1 |
| Timeout | `fh mesh: gmsh timed out after N seconds and was stopped` | 1 |
| Child failure or a line that starts with `Error` | `fh mesh: gmsh failed (exit code N)` or `gmsh failed (stopped by a signal)`, with error text. | 1 |
| Missing output, unreadable mesh, or no volume elements | `fh mesh: gmsh wrote no mesh file: PATH`, `the mesh is not readable (PATH): ERROR`, or `gmsh wrote a mesh with no volume elements: PATH` | 1 |
| An element with a corner Jacobian of 0 or less | `fh mesh: mesh.jacobian.negative: ...` with the count and example element IDs | 1 |
| A volume with no physical group, in a file with physical groups | `fh mesh: mesh.volume.unnamed: ...` | 1 |
| A region of the case that selects no face of the part | `fh mesh: mesh.region.missing: ...` | 1 |
| A region that is not on the outside of the solid | `fh mesh: mesh.region.inner: ...` | 1 |

The child exit code is diagnostic text. `fh mesh` returns 1 for a child failure.
Some operating-system errors supply their own message after the command prefix.

## `fh model`

```text
fh model FILE.msh --case DIR [-o OUT.inp] [--mesh-units SI-mm|SI-m]
```

Source: [model.rs](../../crates/cli/src/model.rs).

The command writes a solver input from the mesh and the `[model]` table of the case (`crates/validation/SCHEMA.md`).
The input starts with the line `** units: SI-mm` or `** units: SI-m`. `fh-solve-ccx` copies the units into `summary.toon` and `results.pvd`. It writes VTU with PVD.
Each material box selects a named solid group and a separate material section.
`model.temperature_change` writes uniform thermal input for CalculiX.
A node set can select a plane or an existing node with `point = [x, y, z]`.
See [case fields](case-toml.md#model-and-thermal-materials).
Each surface region becomes a node set and an element-face surface of its name. Each `[[model.dsload]]` becomes a `*DSLOAD` line.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Success | stdout: tab-separated `ok`, `units`, `nodes`, and `elements` rows. | 0 |
| No unit record and no `--mesh-units` | stderr: `fh model: refused: unit.mesh.missing: ...` | 1 |
| The unit record and `--mesh-units` differ | Same refusal format with `unit.mesh.conflict`. | 1 |
| The mesh units differ from the case | Same refusal format with `unit.mesh.mismatch`. | 1 |
| The case has no `[model]` table | Same refusal format with `model.missing`. | 1 |
| No solid element, or 2 element types | Same refusal format with `mesh.empty` or `mesh.mixed`. | 1 |
| A volume with no physical group | Same refusal format with `mesh.volume.unnamed`. | 1 |
| A region that the mesh does not hold (lost or renamed) | Same refusal format with `mesh.region.missing`. | 1 |
| A region that is not on the outside of the solid | Same refusal format with `mesh.region.inner`. | 1 |
| A material group with no solid element | Same refusal format with `mesh.material.missing`. | 1 |
| An element with zero or several material groups | Same refusal format with `mesh.material.coverage`. | 1 |
| A plane or point set with no node | Same refusal format with `model.set.empty`. | 1 |
| The written input does not parse | Same refusal format with `model.inp.invalid`. | 1 |
| Usage, case, file read, or mesh parse error | `fh model: MESSAGE` | 2 |

## `fh validate`

```text
fh validate [--root DIR]
```

Sources: [validate.rs](../../crates/cli/src/validate.rs),
[measure.rs](../../crates/cli/src/measure.rs), and [harness](../../crates/validation/src/run.rs).

`--root` defaults to `.`. The normal run loads case directories under `DIR/validation/cases`, sorted by directory name.
Each case solves its 1 mesh, then compares each quantity with its reference.
The solver is `FH_SOLVE` when set, otherwise `fh-solve-ccx` beside `fh`, otherwise `fh-solve-ccx` on `PATH`.
The `solver` key of a case can only be `fh-solve-ccx`, the default.
If the solver is `fh-solve-ccx` and `ccx` (`FH_CCX`, else `ccx` on PATH) does not start, `fh validate` stops before the first case.
A case can set `time_limit_s` and `memory_limit_mib` ([case fields](case-toml.md)). At a limit, the runner stops the solver and the case cannot run.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Each case passes | stdout: `ID<TAB>pass`, quantity rows, and `N cases: P pass, F fail`. | 0 |
| `ccx` does not start | stderr: `fh validate: cannot start ccx (PATH): ERROR. fh-solve-ccx needs CalculiX: install it as docs/ccx.md tells, then put ccx on PATH or set FH_CCX` | 1 |
| Case failure | stdout: case row with `fail: REASON`, then summary. | 1 |
| Solver absent or measurement cannot run | stdout: case row with `cannot run: REASON`. | 1 |
| Fixture load error | stderr: `fh validate: FILE[:LINE]: MESSAGE` | 1 |
| Mesh parse error, missing set, or missing step | stderr: `fh validate: CASE.toml: MESSAGE` | 1 |
| Missing option value | `fh validate: --root needs a directory` | 2 |
| Any other argument | `fh validate: unknown option: ARGUMENT` | 2 |
| Empty case directory | `fh validate: no cases under PATH` | 2 |

Quantity rows contain the case ID, quantity name, `computed`, `reference`, `tolerance` and `margin`.
Read or solver failures, absent result fields, empty sets, and invalid reductions can supply the failure reason.
See the [case TOML reference](case-toml.md) for fixture fields and pass rules.

## `fh run`

```text
fh run MODEL.inp|CASE_DIR|PART.step [--size H] [--preview] [--state DIR] [--no-open]
```

Source: [run.rs](../../crates/cli/src/run.rs).

The command solves a model on the daemon of the [run contract](../run-contract.md), shows the events of the run, and opens the run in the browser.

| Input | What `fh run` does |
|---|---|
| `MODEL.inp` (the extension in any case) | Solves the file as it is. |
| `CASE_DIR`, `CASE_DIR/case.toml`, or `CASE_DIR/PART.step` (`.step` or `.stp`) | Runs `fh mesh PART.step --case CASE_DIR` and `fh model` in the work folder `STATE/cases/NAME-HASH`, then solves the input. Thus the case folder does not change. A folder must hold one STEP file. |

| Option | Default or rule |
|---|---|
| `--size H` | Sent to `fh mesh`. A number above 0. The `fh mesh` default for a case folder. |
| `--preview` | Show the model with its supports and loads in the page, then stop. No solve occurs. |
| `--state DIR` | The state folder of the daemon, the same default as `fh serve`. |
| `--no-open` | Do not open the browser. A value of `FH_NO_OPEN` that is not empty and not `0` does the same. |

`fh run` uses the daemon of the state folder when one answers. Else it starts `fh serve --state DIR --no-open` in the background, in its own process group, with its output in `DIR/daemon.log`, and waits up to 20 seconds for it. The file has mode 0600, because it holds the page address with the token. When another daemon answers first, for example the daemon of an `fh run` that started at the same time, `fh run` stops its own `fh serve` and uses the daemon that answers.
That daemon stays alive when `fh run` ends. The browser keeps the results, and the next `fh run` uses the same daemon. `fh serve --stop` stops it.
A daemon that `fh run` starts gets the environment of `fh run`, so it uses `FH_SOLVE` and `FH_VIEWER`. `fh mesh` uses `FH_GMSH`.

The run of a case uses the solver that the `solver` key of `case.toml` names:

- No `solver` key, or `solver = "fh-solve-ccx"`: the solver of the daemon. That is `FH_SOLVE` when set, else `fh-solve-ccx` beside `fh`, else on `PATH`.
- Any other value is an error of the case.
- When `FH_SOLVE` is not set and `fh-solve-ccx` is in neither place, `fh run` stops with `run.solver.missing`.

An `.inp` file uses the solver of the daemon.

Before the solve, `fh run` sends the input to the preview route of the daemon (see [the chat](../chat.md#the-model-before-the-solve)).
The page then shows the model with the faces of each support and each load, the direction of each load, and the names of the regions.
The line `fh run: preview: the page shows NAME (preview ID) with its supports and loads` tells this. NAME is the case folder, or the input file.
The preview and the run each copy the input file, one directly after the other. Thus the page shows the bytes that the solver gets.
With `--preview`, `fh run` prints `fh run: --preview: no solve` and stops. It does not open the browser, because the page that is open shows the model.
A daemon that has no preview route still solves: `fh run` prints `fh run: no preview (MESSAGE)` and continues.

The browser opens on `http://127.0.0.1:PORT/?token=TOKEN&run=R`, the same way as for [`fh serve`](#fh-serve).
Ctrl+C sends a cancel command for the run, then `fh run` waits for the end of the run. A second Ctrl+C ends `fh run` at once. The cancel continues in the daemon.
Before a daemon starts, `fh run` checks the arguments and the input. Thus a usage error starts no daemon.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| `--preview`, and the page shows the model | stdout: the `fh mesh` and `fh model` rows for a case, the preview line, and `fh run: --preview: no solve` | 0 |
| The run succeeded | stdout: the `fh mesh` and `fh model` rows for a case, the preview line, the events of the run, `fh run: RUN succeeded in S s`, `results in DIR`, `summary.toon`, `page URL`, `opened RUN in the browser` (or `no browser (CAUSE)`), and the stop command. | 0 |
| The solver refused the input, or the run failed | stderr: `fh run: RUN failed: MESSAGE`. The message names the input file, not the copy in the state folder, for example `bad.inp: line 3, *BOGUS: unsupported keyword (INP002)`. | 1 |
| The daemon stopped during the run | `fh run: RUN was interrupted: MESSAGE` | 1 |
| `FH_SOLVE` is not set, and `fh-solve-ccx` is not beside `fh` or on `PATH` | `fh run: the solver fh-solve-ccx is not beside fh or on PATH. To install it, see docs/ccx.md (run.solver.missing)`. No daemon starts. | 1 |
| `fh mesh` or `fh model` refused the case | The refusal of that command, for example `fh mesh: unit.step.mismatch: ...`. | 1 |
| `--preview`, and the daemon refused the preview | `fh run: the daemon refused the preview: MESSAGE (CODE)` | 1 |
| The daemon refused the run | `fh run: the daemon refused the run: MESSAGE (CODE)`, for example `queue.full` | 1 |
| No input, unknown option, bad value (for example `--size needs a number above 0`) | `fh run: MESSAGE` and the usage | 2 |
| `--size` with an `.inp` file | `fh run: --size goes to fh mesh, ... (run.option.mesh)` | 2 |
| The input does not exist | `fh run: PATH: ERROR (run.input.missing)` | 2 |
| The input is not an `.inp` file, a folder, `case.toml` or a STEP file | `fh run: PATH: ... (run.input.kind)` | 2 |
| A folder with no `case.toml` | `fh run: DIR: no case.toml, so this is not a case folder (run.case.missing)` | 2 |
| A case folder with no STEP file, or with more than 1 | `(run.case.step.missing)`, or `(run.case.step.many)` with the names | 2 |
| A case that `fh mesh` or `fh model` cannot read | `fh run: mesh: MESSAGE` or `fh run: model: MESSAGE` | 2 |
| The state folder cannot be made | `fh run: no daemon: cannot make the state folder DIR: ERROR` | 3 |
| No daemon answers and `fh serve` does not start one | `fh run: no daemon: fh serve did not start a daemon for DIR`, then the last 5 lines of `DIR/daemon.log`, for example `another daemon uses this state folder (daemon.running)` | 3 |
| The daemon does not answer for 10 seconds during the run | `fh run: RUN: the daemon does not answer: ...` | 3 |
| Ctrl+C, and the run ended as cancelled | stdout: `cancel: cancelling` (the state after the cancel). stderr: `fh run: RUN was cancelled` | 130 |
| A second Ctrl+C | stdout: `fh run: Ctrl+C again: fh run ends. The cancel of RUN continues in the daemon.` | 130 |

`crates/cli/tests/run.rs` has a test for each refusal, with a fake browser, `FH_NO_OPEN=1`, and a fake gmsh.

## `fh serve`

```text
fh serve [--state DIR] [--port N] [--no-open]
fh serve --stop [--state DIR]
```

Source: [serve.rs](../../crates/daemon/src/serve.rs). The dispatcher calls it with the solver of `fh validate`.

The command runs the daemon of the [run contract](../run-contract.md) on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP.
`--state` defaults to `$FH_STATE`, else `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. `fh run` has the same default.
`--port` defaults to 0, a free port. The solver is `FH_SOLVE` when set, otherwise `fh-solve-ccx` beside `fh`, otherwise on `PATH`. It needs CalculiX ([ccx](../ccx.md)).
The page is the wasm viewer ([viewer](../viewer.md)), from `FH_VIEWER` when set, otherwise the `viewer` folder beside `fh`.
The page has the chat with the agent ([chat](../chat.md)).

After the start, `fh serve` opens its page in the browser. `--no-open`, or a value of `FH_NO_OPEN` that is not empty and not `0`, stops this, for tests and for a machine with no screen.
The address holds the token, so it does not go into the command line of a browser process, where other users of the machine can read it:

| Browser program | What it gets |
|---|---|
| `$BROWSER`, when it is set | the file `STATE/open.html` (mode 0600), which sends the browser on to the address |
| macOS | `open URL`: Launch Services gives the address to the default browser |
| Other Unix systems | `xdg-open STATE/open.html` |

`fh serve --stop` sends SIGTERM to the daemon that answers for the state folder, then waits up to 30 seconds until the daemon releases `daemon.lock`.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Start | stdout: `fh serve: http://127.0.0.1:PORT/?token=TOKEN`, the viewer folder (or a line that says the wasm viewer is not built), the state folder, and `fh serve: opened the page in the browser` (or `no browser (CAUSE)`). `DIR/daemon.info` gets the port and the token. | |
| Stop signal | stdout: `fh serve: stopped`. `daemon.info` is removed. | 0 |
| The daemon cannot start | stderr: `fh serve: DIR: MESSAGE (CODE)`, for example `daemon.running`. When another daemon answers: `its page is URL. Stop it with: fh serve --stop --state DIR` | 1 |
| `--stop` stopped the daemon | stdout: `fh serve: stopped the daemon (pid PID) of DIR` | 0 |
| `--stop` and no daemon answers | stdout: `fh serve: no daemon answers for DIR (CODE)` | 0 |
| `--stop` and the daemon does not stop in 30 seconds | stderr: `fh serve: the daemon (pid PID) did not stop in 30 s` | 1 |
| Missing option value | `fh serve: OPTION needs a value` | 2 |
| Bad port | `fh serve: --port needs a number from 0 to 65535` | 2 |
| `--stop` with an option other than `--state` | `fh serve: --stop takes only --state DIR` | 2 |
| Any other argument | `fh serve: unknown option: ARGUMENT` | 2 |

## Keep this list current

Run `fh --help` from the branch under review after a CLI change.
Compare dispatch and exit codes with `main.rs` and every command’s `run` function.
Check returned errors as well as explicit exit codes. The dispatcher converts returned errors to exit code 2.
