# `fh` commands, diagnostics, and exit codes

`fh` implements `mesh`, `validate`, `run`, and `serve`.
This reference comes from the [command dispatcher](../../crates/cli/src/main.rs) and the command sources below.
The help text was checked with `cargo +stable run -q -p fh-cli --bin fh -- --help`.

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
fh mesh FILE.(geo|step) --units SI-mm|SI-m [-o OUT.inp] [--size H] [--scale S] [--set NAME=VALUE]...
  [--face NAME=SEL]... [--nodes NAME=SEL]... [--volume NAME=box:X0,Y0,Z0,X1,Y1,Z1]...
  [--timeout SECONDS] [--gmsh PATH]
```

Source: [mesh.rs](../../crates/cli/src/mesh.rs).

The command accepts `.geo`, `.step`, and `.stp`, without regard to extension case.
It runs external gmsh for a 3D mesh of order 2 (`C3D10`) in `msh41` format, then writes the nodes, the elements and the named sets as a CalculiX input.
The input has no material, no step and no load. The chat includes it with `*INCLUDE` in the input that it writes.

| Option | Default or rule |
|---|---|
| `-o` | Input path with the extension changed to `.inp`. |
| `--size` | No value by default. Positive finite number, sent as gmsh `-clmax`. |
| `--scale` | No value by default. Positive finite number, sent as gmsh `-clscale`. |
| `--set` | Repeatable. Name uses ASCII letters, digits, `_`, or `.`. Value is finite. Sent as `-setnumber`. |
| `--units` | Required: `SI-mm` or `SI-m`. STEP units must match. The command writes `** units: SI-mm` (or `SI-m`) into the output. |
| `--face NAME=SEL` | Repeatable. A node set `NAME` and a surface `NAME` of the faces of the part that lie fully inside the selection, within 1e-6 of the diagonal of the part. |
| `--nodes NAME=SEL` | Repeatable. A node set of the nodes in the selection. |
| `--volume NAME=box:...` | Repeatable. STEP only. An element set of the elements in the box. Each element must be in exactly 1 box. |
| `--timeout` | No limit by default. Positive finite duration. The version probe has a separate 30-second limit. |
| `--gmsh` | Overrides discovery. Otherwise use non-empty `FH_GMSH`, then a file on `PATH`, then the macOS app path. |

`SEL` is a plane (`x:V`, `y:V` or `z:V`), a box (`box:X0,Y0,Z0,X1,Y1,Z1`) or, for `--nodes` only, a point (`point:X,Y,Z`).
A name uses letters, digits and `_`, and not `ALL` or `NALL`. Each name is used once.
`NALL` is the node set of all nodes. `ALL` is the element set of all elements.

For a STEP file, `fh mesh` writes the gmsh script (the fixed script contract of M2b).
The script sets the STEP target unit and uses `SOLID` for one material without a `--volume` box.
For `--volume` boxes, it fragments the bodies and creates one physical volume group per box.
It checks that each solid element belongs to exactly one box.

The macOS fallback is `/Applications/Gmsh.app/Contents/MacOS/gmsh`.
The minimum gmsh version is 4.8.
The command replaces an existing output file only when the mesh succeeds.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Mesh success | stdout: tab-separated `ok`, `gmsh`, `units`, `nodes`, `elements`, `min_scaled_jacobian`, `type`, `nset`, `elset`, and `surface` (element-face count) rows. | 0 |
| gmsh warning or different release | stdout: `warning<TAB>MESSAGE`. gmsh lines that start with `Warning` are retained. | 0 |
| Missing option value, invalid number, selection, units, or name | stderr: `fh mesh: MESSAGE` | 2 |
| Unknown option, no input, or more than 1 input | Same prefix. Includes mesh usage. | 2 |
| Input read, extension, path, or temporary-file error | `fh mesh: MESSAGE` | 1 |
| Refused `.geo` command | Same prefix. Refuses `SystemCall`, `NonBlockingSystemCall`, `OnelabRun`, `Include`, and non-`.msh` or non-literal `Merge`. | 1 |
| No `--units` | `fh mesh: unit.missing: ...` | 1 |
| No STEP length record | `fh mesh: unit.step.missing: ...` | 1 |
| Multiple STEP length units | `fh mesh: unit.step.conflict: ...` | 1 |
| STEP unit differs from `--units` | `fh mesh: unit.step.mismatch: ...` | 1 |
| gmsh not found or cannot start | `fh mesh: gmsh is not installed. ...` or `cannot start gmsh (PATH): ERROR` | 1 |
| Unreadable or old version | `fh mesh: cannot read the gmsh version from ...` or `gmsh VERSION is too old, ...` | 1 |
| Timeout | `fh mesh: gmsh timed out after N seconds and was stopped` | 1 |
| Child failure or a line that starts with `Error` | `fh mesh: gmsh failed (exit code N)` or `gmsh failed (stopped by a signal)`, with error text. | 1 |
| Missing output, unreadable mesh, or no volume elements | `fh mesh: gmsh wrote no mesh file: PATH`, `the mesh is not readable (PATH): ERROR`, or `gmsh wrote a mesh with no volume elements: PATH` | 1 |
| An element with a corner Jacobian of 0 or less | `fh mesh: mesh.jacobian.negative: ...` with the count and example element IDs | 1 |
| A volume with no physical group, in a file with physical groups | `fh mesh: mesh.volume.unnamed: ...` | 1 |
| A `--face` that selects no whole face of the part | `fh mesh: mesh.face.missing: ...` | 1 |
| A `--face` that is not on the outside of the solid | `fh mesh: mesh.face.inner: ...` | 1 |
| A `--nodes` selection with no node | `fh mesh: mesh.set.empty: ...` | 1 |
| A name used twice | `fh mesh: mesh.name.duplicate: ...` | 1 |
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
fh run MODEL.inp [--preview] [--timeout SECONDS] [--memory MIB] [--state DIR] [--no-open]
```

Source: [run.rs](../../crates/cli/src/run.rs).

The command solves a model on the daemon, shows the events of the run, and opens the run in the browser.

`MODEL.inp` is a CalculiX input file (the extension in any case). `fh run` gives it to `ccx` as it is, with each file that it includes (`*INCLUDE`). The run solves a copy, so a later edit of the file does not change the run.

| Option | Default or rule |
|---|---|
| `--timeout SECONDS` | No limit by default. A number above 0. At the limit, the daemon stops `ccx`. |
| `--memory MIB` | No limit by default. A number above 0. At the limit, the daemon stops `ccx`. |
| `--preview` | Show the model with its supports and loads in the page, then stop. No solve occurs. |
| `--state DIR` | The state folder of the daemon, the same default as `fh serve`. |
| `--no-open` | Do not open the browser. A value of `FH_NO_OPEN` that is not empty and not `0` does the same. |

`fh run` uses the daemon of the state folder when one answers. Else it starts `fh serve --state DIR --no-open` in the background, in its own process group, with its output in `DIR/daemon.log`, and waits up to 20 seconds for it. The file has mode 0600, because it holds the page address with the token. When another daemon answers first, for example the daemon of an `fh run` that started at the same time, `fh run` stops its own `fh serve` and uses the daemon that answers.
That daemon stays alive when `fh run` ends. The browser keeps the results, and the next `fh run` uses the same daemon. `fh serve --stop` stops it.
A daemon that `fh run` starts gets the environment of `fh run`, so it uses `FH_CCX` and `FH_VIEWER`.

Before the solve, `fh run` sends the input to the preview route of the daemon.
The page then shows the model with the faces of each support and each load, the direction of each load, and the names of the sets.
The line `fh run: preview: the page shows NAME (preview ID) with its supports and loads` tells this. NAME is the input file.
The preview and the run each copy the input file, one directly after the other. Thus the page shows the bytes that the solver gets.
With `--preview`, `fh run` prints `fh run: --preview: no solve` and stops. It does not open the browser, because the page that is open shows the model.
A daemon that has no preview route still solves: `fh run` prints `fh run: no preview (MESSAGE)` and continues.

The browser opens on `http://127.0.0.1:PORT/?token=TOKEN&run=R`, the same way as for [`fh serve`](#fh-serve).
Ctrl+C sends a cancel command for the run, then `fh run` waits for the end of the run. The cancel stops `ccx` at once. A second Ctrl+C ends `fh run` at once. The cancel continues in the daemon.
Before a daemon starts, `fh run` checks the arguments and the input. Thus a usage error starts no daemon.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| `--preview`, and the page shows the model | stdout: the preview line and `fh run: --preview: no solve` | 0 |
| The run succeeded | stdout: the preview line, the events of the run, `fh run: RUN succeeded in S s`, `results in DIR`, the paths of `job.dat` and `job.sta`, `page URL`, `opened RUN in the browser` (or `no browser (CAUSE)`), and the stop command. When the solve wrote no field, a line says that the page has no field to show. | 0 |
| `ccx` refused the input, or the run failed | stderr: `fh run: RUN failed: MESSAGE`. The message names the input file, not the copy in the state folder. | 1 |
| The daemon stopped during the run | `fh run: RUN was interrupted: MESSAGE` | 1 |
| `--preview`, and the daemon refused the preview | `fh run: the daemon refused the preview: MESSAGE (CODE)` | 1 |
| The daemon refused the run | `fh run: the daemon refused the run: MESSAGE (CODE)`, for example `queue.full` | 1 |
| No input, unknown option, bad value (for example `--timeout needs a number above 0`) | `fh run: MESSAGE` and the usage | 2 |
| The input does not exist | `fh run: PATH: ERROR (run.input.missing)` | 2 |
| The input is not an `.inp` file | `fh run: PATH: fh run takes a CalculiX input file (.inp) (run.input.kind)` | 2 |
| The state folder cannot be made | `fh run: no daemon: cannot make the state folder DIR: ERROR` | 3 |
| No daemon answers and `fh serve` does not start one | `fh run: no daemon: fh serve did not start a daemon for DIR`, then the last 5 lines of `DIR/daemon.log`, for example `another daemon uses this state folder (daemon.running)` | 3 |
| The daemon does not answer for 10 seconds during the run | `fh run: RUN: the daemon does not answer: ...` | 3 |
| Ctrl+C, and the run ended as cancelled | stdout: `cancel: cancelling` (the state after the cancel). stderr: `fh run: RUN was cancelled` | 130 |
| A second Ctrl+C | stdout: `fh run: Ctrl+C again: fh run ends. The cancel of RUN continues in the daemon.` | 130 |

`crates/cli/tests/run.rs` has a test for each refusal, with a fake browser and `FH_NO_OPEN=1`.

## `fh serve`

```text
fh serve [--state DIR] [--port N] [--no-open]
fh serve --stop [--state DIR]
```

Source: [serve.rs](../../crates/daemon/src/serve.rs).

The command runs the daemon on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP.
`--state` defaults to `$FH_STATE`, else `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. `fh run` has the same default.
`--port` defaults to 0, a free port. The solver is `ccx` (`FH_CCX`, else `ccx` on `PATH`).
The page is the wasm viewer, from `FH_VIEWER` when set, otherwise the `viewer` folder beside `fh`.
The page has the chat with the agent.

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
