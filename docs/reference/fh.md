# `fh` commands, diagnostics, and exit codes

`fh` implements `check`, `mesh`, `model`, `validate`, `info`, `run`, `serve`, and `view`.
This reference comes from the [command dispatcher](../../crates/cli/src/main.rs) and the command sources below.
The help text was checked with `cargo +stable run -q -p fh-cli --bin fh -- --help`.

## Dispatch and help

| Invocation | Output | Exit |
|---|---|---:|
| `fh`, `fh -h`, `fh --help` | Usage and command list on stdout. | 0 |
| `fh results`, `plot`, `init`, `team`, or `skills` | `fh COMMAND: not implemented` and usage on stderr. | 2 |
| Unknown command or invalid top-level arguments | `fh: unknown argument: ARGUMENTS` and usage on stderr. | 2 |

A command error returned to the dispatcher prints `fh COMMAND: MESSAGE` on stderr and exits with 2.
This includes command argument errors. Subcommands do not implement `-h` or `--help`.
The help text mentions TOON precision, but these command handlers print the formats listed below.
The command sources define more `mesh` options than the top-level help text shows.

## `fh check`

```text
fh check [--root DIR] [FILE.inp...]
```

Source: [check.rs](../../crates/cli/src/check.rs).

Explicit files take precedence over discovery. With no files, the command searches `fixtures/` and `validation/cases/` under `DIR`.
`DIR` defaults to `.`. Discovery is recursive, uses the exact `.inp` extension, and sorts file paths.
A missing discovery directory contributes no files.
A path with adjacent `validation/cases` components also gets a solid-section coverage check.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Each file passes | stdout: `ok<TAB>FILE<TAB>N nodes, E elements, S steps` | 0 if all pass |
| A file cannot be read or parsed | stderr: `fail<TAB>FILE<TAB>MESSAGE` | 1 |
| A case mesh has an uncovered element | Same failure format, with `element ID has no *SOLID SECTION` | 1 |
| End of file checks | stdout: `N files: P ok, F failed` | 0 or 1 |
| Missing `--root` value | `fh check: --root needs a directory` | 2 |
| Unknown option | `fh check: unknown option: OPTION` | 2 |
| No discovered files | `fh check: no .inp fixtures found under DIR` | 2 |
| Discovery read error | `fh check: cannot read PATH: ERROR` or `cannot read an entry of DIR: ERROR` | 2 |

The command continues after a file failure. Parser messages contain a line number and, when known, a keyword.
See the [input reference](inp.md) for accepted keywords and parser limits.

## `fh mesh`

```text
fh mesh FILE.(geo|step) (--case DIR | --units SI-mm|SI-m) [-o OUT.msh] [--order 1|2]
  [--size H] [--scale S] [--set NAME=VALUE]... [--timeout SECONDS] [--gmsh PATH]
```

Source: [mesh.rs](../../crates/cli/src/mesh.rs).

The command accepts `.geo`, `.step`, and `.stp`, without regard to extension case.
It runs external gmsh for a 3D mesh in `msh41` format, then reads the result.

| Option | Default or rule |
|---|---|
| `-o` | Input path with the extension changed to `.msh`. |
| `--order` | `2`. Only `1` and `2` are accepted. |
| `--size` | No value by default. Positive finite number, sent as gmsh `-clmax`. |
| `--scale` | No value by default. Positive finite number, sent as gmsh `-clscale`. |
| `--set` | Repeatable. Name uses ASCII letters, digits, `_`, or `.`. Value is finite. Sent as `-setnumber`. |
| `--units` | `SI-mm`. Also accepts `SI-m`. STEP units must match. |
| `--timeout` | 300 seconds. Positive finite duration. The version probe has a separate 30-second limit. |
| `--gmsh` | Overrides discovery. Otherwise use non-empty `FH_GMSH`, then a file on `PATH`, then the macOS app path. |

For a STEP file, `fh mesh` writes the gmsh script (the fixed script contract of M2b).
The script sets the STEP target unit, puts every volume in the physical group `SOLID`,
and makes a physical surface for each `[[model.region]]` of the case.
A region holds the faces of the part that lie fully inside its plane or box, within 1e-6 of the diagonal of the part.

The macOS fallback is `/Applications/Gmsh.app/Contents/MacOS/gmsh`.
The minimum gmsh version is 4.8. A release outside 4.15 produces a warning.
The command removes an existing output file before the mesh run.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Mesh success | stdout: tab-separated `ok`, `gmsh`, `units`, `order`, `nodes`, `elements`, `type`, `nset`, `elset`, and `surface` (element-face count) rows. | 0 |
| gmsh warning or different release | stdout: `warning<TAB>MESSAGE`. gmsh lines that start with `Warning` are retained. | 0 |
| Missing option value, invalid number, order, units, or set name | stderr: `fh mesh: MESSAGE` | 2 |
| Unknown option, no input, or more than 1 input | Same prefix. Includes mesh usage. | 2 |
| Cannot create the temporary run directory | `fh mesh: cannot make PATH: ERROR` | 2 |
| Input read, extension, path, or temporary-file error | `fh mesh: MESSAGE` | 1 |
| Refused `.geo` command | Same prefix. Refuses `SystemCall`, `NonBlockingSystemCall`, `OnelabRun`, `Include`, and non-`.msh` or non-literal `Merge`. | 1 |
| No STEP length record | `fh mesh: unit.step.missing: ...` | 1 |
| Multiple STEP length units | `fh mesh: unit.step.mixed: ...` | 1 |
| STEP unit differs from declared units | `fh mesh: unit.step.mismatch: ...` | 1 |
| gmsh not found or cannot start | `fh mesh: gmsh is not installed. ...` or `cannot start gmsh (PATH): ERROR` | 1 |
| Unreadable or old version | `fh mesh: cannot read the gmsh version from ...` or `gmsh VERSION is too old, ...` | 1 |
| Cannot replace output | `fh mesh: cannot replace PATH: ERROR` | 1 |
| Timeout | `fh mesh: gmsh timed out after N seconds and was stopped` | 1 |
| Child failure or a line that starts with `Error` | `fh mesh: gmsh failed (exit code N)` or `gmsh failed (stopped by a signal)`, with error text. | 1 |
| Missing output, unreadable mesh, or no volume elements | `fh mesh: gmsh wrote no mesh file: PATH`, `the mesh is not readable (PATH): ERROR`, or `gmsh wrote a mesh with no volume elements: PATH` | 1 |
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
The input starts with the line `** units: SI-mm` or `** units: SI-m`. `fh-solve` copies the units into `summary.toon`, `results.pvd` and the text results.
Each region becomes a node set and an element-face surface of its name. Each `[[model.dsload]]` becomes a `*DSLOAD` line.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Success | stdout: tab-separated `ok`, `units`, `nodes`, and `elements` rows. | 0 |
| No unit record and no `--mesh-units` | stderr: `fh model: refused: unit.mesh.missing: ...` | 1 |
| The unit record and `--mesh-units` differ | Same refusal format with `unit.mesh.conflict`. | 1 |
| The mesh units differ from the case | Same refusal format with `unit.mesh.mismatch`. | 1 |
| The case has no `[model]` table | Same refusal format with `model.missing`. | 1 |
| No solid element, or 2 element types | Same refusal format with `mesh.empty` or `mesh.mixed`. | 1 |
| A hex8 mesh that is not one extrusion | Same refusal format with `mesh.hex.not_extruded`. | 1 |
| A volume with no physical group | Same refusal format with `mesh.volume.unnamed`. | 1 |
| A region that the mesh does not hold (lost or renamed) | Same refusal format with `mesh.region.missing`. | 1 |
| A region that is not on the outside of the solid | Same refusal format with `mesh.region.inner`. | 1 |
| A plane set with no node | Same refusal format with `model.set.empty`. | 1 |
| The written input does not parse | Same refusal format with `model.inp.invalid`. | 1 |
| Usage, case, file read, or mesh parse error | `fh model: MESSAGE` | 2 |

## `fh info`

```text
fh info FILE.step|FILE.msh [--case DIR]
```

Sources: [info.rs](../../crates/cli/src/info.rs), [STEP unit reader](../../crates/io/src/step.rs),
and [mesh statistics](../../crates/io/src/check.rs).

The accepted extensions are exactly `.step`, `.stp`, and `.msh` in lower case.
`--case DIR` loads `DIR/case.toml`. It checks STEP units or labels mesh lengths with the declared unit.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| STEP scan succeeds | stdout: `file`, `kind: step`, `step_unit`. With a matching case: `case_units`, `units_match: yes`, `gmsh_target_unit`. | 0 |
| Mesh scan succeeds | stdout: `file`, `kind: msh`, `nodes`, `elements`, `bbox_min`, `bbox_max`, `volume`, `min_scaled_jacobian`. | 0 |
| No STEP length record | stderr: `fh info: refused: unit.step.missing: MESSAGE` | 1 |
| STEP records have different units | Same refusal format with `unit.step.conflict`. | 1 |
| STEP unit differs from case | Same refusal format with `unit.step.mismatch`. | 1 |
| No supported solid elements | Same refusal format with `mesh.empty`. | 1 |
| Corner Jacobian is 0 or less | Same refusal format with `mesh.jacobian.negative`. Includes the count and example element IDs. | 1 |
| No file | `fh info: usage: fh info FILE.step|FILE.msh [--case DIR]` | 2 |
| Missing `--case` value | `fh info: --case needs a directory` | 2 |
| Unknown option or extra argument | `fh info: unknown option: OPTION` or `unexpected argument: ARGUMENT` | 2 |
| Case, file read, mesh parse, statistics, or extension error | `fh info: MESSAGE` | 2 |

Without a case, a single STEP unit can pass even when it is not `mm` or `m`.
Mesh statistics can precede a Jacobian refusal. A refusal does not imply empty stdout.

## `fh validate`

```text
fh validate [--root DIR] [--convergence PATH]
```

Sources: [validate.rs](../../crates/cli/src/validate.rs),
[measure.rs](../../crates/cli/src/measure.rs), and [harness](../../crates/validation/src/run.rs).

`--root` defaults to `.`. The normal run loads case directories under `DIR/validation/cases`, sorted by directory name.
`--convergence PATH` takes precedence over `--root`. It accepts a case directory or a directory of case directories.
A directory run skips class N cases. A class N case named alone fails.
A class R case solves each mesh with boundary values from the exact field of the fixture.
It reports one row, `rate`, with the observed rate, the expected rate, the band and the error of each mesh.
The case fails if the error does not fall from one mesh to the next.
The solver is `FH_SOLVE` when set, otherwise `fh-solve` beside `fh`.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| All executed cases pass | stdout: `ID<TAB>CLASS<TAB>pass`, quantity rows, and `N cases: P pass, F fail`. | 0 |
| Convergence directory contains class N | stdout: `ID<TAB>N<TAB>skipped, one mesh`. Skipped cases count in the total, not pass or fail. | 0 if no failure |
| Case failure | stdout: case row with `fail: REASON`, then summary. Convergence failures also print `fh validate: REASON` on stderr. | 1 |
| Solver absent or measurement cannot run | stdout: case row with `cannot run: REASON` or `cannot run: no solver`. | 1 |
| Fixture load error | stderr: `fh validate: FILE[:LINE]: MESSAGE` | 1 |
| Normal-run mesh parse, undeclared element type, missing set, or missing step | stderr: `fh validate: CASE.toml: MESSAGE` | 1 |
| Missing option value | `fh validate: --root needs a directory` or `--convergence needs a path` | 2 |
| Any other argument | `fh validate: unknown option: ARGUMENT` | 2 |
| Empty case directory | `fh validate: no cases under PATH` | 2 |

Quantity rows contain the case ID, quantity name, `computed`, `reference`, `tolerance`, `margin`, and a note.
Convergence case rows also contain the mesh count.
Read or solver failures, absent result fields, empty sets, and invalid reductions can supply the failure reason.
See the [case TOML reference](case-toml.md) for fixture fields and pass rules.

## `fh run`

```text
fh run MODEL.inp|CASE_DIR|PART.step [--size H] [--order 1|2] [--state DIR] [--panes auto|herdr|pty] [--no-open]
fh run --example NAME [--size H] [--order 1|2] [--state DIR] [--panes auto|herdr|pty] [--no-open]
```

Source: [run.rs](../../crates/cli/src/run.rs).

The command solves a model on the daemon of the [run contract](../run-contract.md), shows the events of the run, and opens the run in the browser.

| Input | What `fh run` does |
|---|---|
| `MODEL.inp` (the extension in any case) | Solves the file as it is. The folder of the file is the project root. |
| `CASE_DIR`, `CASE_DIR/case.toml`, or `CASE_DIR/PART.step` (`.step` or `.stp`) | Runs `fh mesh PART.step --case CASE_DIR` and `fh model` in the work folder `STATE/cases/NAME-HASH`, then solves the input. The work folder is the project root, so the case folder does not change. A folder must hold one STEP file. |
| `--example NAME` | Writes an example case that `fh` holds to `STATE/examples/NAME`, then runs it as a case folder. It works from any folder. The example is `bar-pressure` (mesh size 8). |

| Option | Default or rule |
|---|---|
| `--size H` | Sent to `fh mesh`. A number above 0. The `fh mesh` default for a case folder, 8 for `bar-pressure`. |
| `--order 1\|2` | Sent to `fh mesh`. The `fh mesh` default (2). |
| `--state DIR` | The state folder of the daemon, the same default as `fh serve`. |
| `--panes auto\|herdr\|pty` | Sent to a daemon that `fh run` starts, as `fh serve --panes`. The `fh serve` default (`auto`). A daemon that runs keeps its pane backend. |
| `--no-open` | Do not open the browser. A value of `FH_NO_OPEN` that is not empty and not `0` does the same. |

`fh run` uses the daemon of the state folder when one answers. Else it starts `fh serve --state DIR --no-open` (and `--panes MODE` when given) in the background, in its own process group, with its output in `DIR/daemon.log`, and waits up to 20 seconds for it.
That daemon stays alive when `fh run` ends. The browser keeps the results, and the next `fh run` uses the same daemon. `fh serve --stop` stops it.
A daemon that `fh run` starts gets the environment of `fh run`, so it uses `FH_SOLVE` and `FH_VIEWER`. `fh mesh` uses `FH_GMSH`.

The browser opens on `http://127.0.0.1:PORT/?token=TOKEN&project=P&run=R`, the same way as for [`fh serve`](#fh-serve).
Ctrl+C sends a cancel command for the run, then `fh run` waits for the end of the run. A second Ctrl+C ends `fh run` at once. The cancel continues in the daemon.
Before a daemon starts, `fh run` checks the arguments and the input. Thus a usage error starts no daemon.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| The run succeeded | stdout: the `fh mesh` and `fh model` rows for a case, the events of the run, `fh run: RUN succeeded in S s`, `results in DIR`, `summary.toon`, `page URL`, `opened RUN in the browser` (or `no browser (CAUSE)`), `native window: fh view DIR`, and the stop command. | 0 |
| The solver refused the input, or the run failed | stderr: `fh run: RUN failed: MESSAGE`. The message names the input file, not the copy in the state folder, for example `bad.inp: line 3, *BOGUS: unsupported keyword (INP002)`. | 1 |
| The daemon stopped during the run | `fh run: RUN was interrupted: MESSAGE` | 1 |
| `fh mesh` or `fh model` refused the case | The refusal of that command, for example `fh mesh: unit.step.mismatch: ...`. | 1 |
| The daemon refused the project or the run | `fh run: the daemon refused the project: MESSAGE (CODE)` or `the daemon refused the run: ...`, for example `queue.full` | 1 |
| No input, unknown option, bad value (for example `--panes is auto, herdr or pty, not VALUE`), or a model and `--example` together | `fh run: MESSAGE` and the usage | 2 |
| `--size` or `--order` with an `.inp` file | `fh run: --size and --order go to fh mesh, ... (run.option.mesh)` | 2 |
| The input does not exist | `fh run: PATH: ERROR (run.input.missing)` | 2 |
| The input is not an `.inp` file, a folder, `case.toml` or a STEP file | `fh run: PATH: ... (run.input.kind)` | 2 |
| A folder with no `case.toml` | `fh run: DIR: no case.toml, so this is not a case folder (run.case.missing)` | 2 |
| A case folder with no STEP file, or with more than 1 | `(run.case.step.missing)`, or `(run.case.step.many)` with the names | 2 |
| An unknown example | `fh run: unknown example NAME. The examples: bar-pressure (run.example.unknown)` | 2 |
| A case that `fh mesh` or `fh model` cannot read | `fh run: mesh: MESSAGE` or `fh run: model: MESSAGE` | 2 |
| The state folder cannot be made | `fh run: no daemon: cannot make the state folder DIR: ERROR` | 3 |
| No daemon answers and `fh serve` does not start one | `fh run: no daemon: fh serve did not start a daemon for DIR`, then the last 5 lines of `DIR/daemon.log`, for example `another daemon uses this state folder (daemon.running)` | 3 |
| The daemon does not answer for 10 seconds during the run | `fh run: RUN: the daemon does not answer: ...` | 3 |
| Ctrl+C, and the run ended as cancelled | stdout: `cancel: cancel-requested`. stderr: `fh run: RUN was cancelled` | 130 |
| A second Ctrl+C | stdout: `fh run: Ctrl+C again: fh run ends. The cancel of RUN continues in the daemon.` | 130 |

`crates/cli/tests/run.rs` has a test for each refusal, with a fake browser, `FH_NO_OPEN=1`, a fake gmsh and `--panes pty`.

## `fh serve`

```text
fh serve [--state DIR] [--port N] [--panes auto|herdr|pty] [--herdr-session NAME] [--herdr PATH] [--no-open]
fh serve --stop [--state DIR]
```

Source: [serve.rs](../../crates/daemon/src/serve.rs). The dispatcher calls it with the solver of `fh validate`.

The command runs the daemon of the [run contract](../run-contract.md) on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP.
`--state` defaults to `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy` (`%LOCALAPPDATA%\femahoy` on Windows).
`--port` defaults to 0, a free port. The solver is `FH_SOLVE` when set, otherwise `fh-solve` beside `fh`.
The page is the wasm viewer ([viewer](../viewer.md)), from `FH_VIEWER` when set, otherwise the `viewer` folder beside `fh`.
The page has the terminal sidebar ([terminal](../terminal.md)).
`--panes` selects the pane backend: `auto` (the default) uses herdr when it answers, else plain PTYs of the daemon.
`--herdr-session` names the herdr session that femahoy owns (default `femahoy`, never `default`). `--herdr` gives the herdr program.

After the start, `fh serve` opens its page in the browser. `--no-open`, or a value of `FH_NO_OPEN` that is not empty and not `0`, stops this, for tests and for a machine with no screen.
The address holds the token, so it does not go into the command line of a browser process, where other users of the machine can read it:

| Browser program | What it gets |
|---|---|
| `$BROWSER`, when it is set | the file `STATE/open.html` (mode 0600), which sends the browser on to the address |
| macOS | `open URL`: Launch Services gives the address to the default browser |
| Windows | `rundll32 url.dll,FileProtocolHandler URL` |
| Other Unix systems | `xdg-open STATE/open.html` |

`fh serve --stop` sends SIGTERM to the daemon that answers for the state folder (`taskkill` on Windows), then waits up to 30 seconds until the daemon releases `daemon.lock`.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Start | stdout: `fh serve: http://127.0.0.1:PORT/?token=TOKEN`, the viewer folder (or a line that says the wasm viewer is not built), the pane backend (for example `fh serve: panes pty (plain PTY panes owned by the daemon)`), the state folder, and `fh serve: opened the page in the browser` (or `no browser (CAUSE)`). `DIR/daemon.info` gets the port and the token. | |
| Stop signal | stdout: `fh serve: stopped`. `daemon.info` is removed. | 0 |
| The daemon cannot start | stderr: `fh serve: DIR: MESSAGE (CODE)`, for example `daemon.running`, `pane.herdr.session` (`--herdr-session default`), `pane.herdr.absent` (`--panes herdr` and no herdr on `PATH`) or `pane.herdr` (herdr does not answer). When another daemon answers: `its page is URL. Stop it with: fh serve --stop --state DIR` | 1 |
| `--stop` stopped the daemon | stdout: `fh serve: stopped the daemon (pid PID) of DIR` | 0 |
| `--stop` and no daemon answers | stdout: `fh serve: no daemon answers for DIR (CODE)` | 0 |
| `--stop` and the daemon does not stop in 30 seconds | stderr: `fh serve: the daemon (pid PID) did not stop in 30 s` | 1 |
| Missing option value | `fh serve: OPTION needs a value` | 2 |
| Bad port | `fh serve: --port needs a number from 0 to 65535` | 2 |
| Bad pane backend | `fh serve: --panes is auto, herdr or pty, not VALUE` | 2 |
| `--stop` with an option other than `--state` | `fh serve: --stop takes only --state DIR` | 2 |
| Any other argument | `fh serve: unknown option: ARGUMENT` | 2 |

## `fh view`

```text
fh view MODEL.inp|RUN_DIR|results.pvd
```

Source: [view.rs](../../crates/cli/src/view.rs) and [crates/viewer](../../crates/viewer/src/lib.rs).
The command opens a native window on the mesh of an `.inp` file, or on the results of a run folder ([viewer](../viewer.md)).
An argument that is a folder or ends in `.pvd` is a run. Any other argument is an `.inp` file.

| Condition | Diagnostic | Exit |
|---|---|---:|
| Window closed | none | 0 |
| No argument, or more than one | `fh view: usage: fh view MODEL.inp\|RUN_DIR\|results.pvd` | 2 |
| An argument that starts with `-` | `fh view: unknown option: ARGUMENT` | 2 |
| An `.inp` file with no element to draw | `fh view: FILE: no C3D4, C3D8 or C3D10 elements to draw (N nodes, M elements)` | 2 |
| Bad or missing result files | `fh view: RUN_DIR: FILE: MESSAGE (CODE)`, with the codes of [viewer](../viewer.md#result-files) | 2 |

## Keep this list current

Run `fh --help` from the branch under review after a CLI change.
Compare dispatch and exit codes with `main.rs` and every command’s `run` function.
Check refusal identifiers in both STEP readers. `mesh` uses `unit.step.mixed`, while `info` uses `unit.step.conflict`.
Check returned errors as well as explicit exit codes. The dispatcher converts returned errors to exit code 2.
