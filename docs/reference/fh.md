# `fh` commands, diagnostics, and exit codes

`fh` implements `check`, `mesh`, `validate`, `info`, and `serve`.
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
fh mesh FILE.(geo|step) [-o OUT.msh] [--order 1|2] [--size H]
  [--scale S] [--set NAME=VALUE]... [--units SI-mm|SI-m]
  [--timeout SECONDS] [--gmsh PATH]
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

The macOS fallback is `/Applications/Gmsh.app/Contents/MacOS/gmsh`.
The minimum gmsh version is 4.8. A release outside 4.15 produces a warning.
The command removes an existing output file before the mesh run.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Mesh success | stdout: tab-separated `ok`, `gmsh`, `units`, `order`, `nodes`, `elements`, `type`, `nset`, and `elset` rows. | 0 |
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

The child exit code is diagnostic text. `fh mesh` returns 1 for a child failure.
Some operating-system errors supply their own message after the command prefix.

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

## `fh serve`

```text
fh serve [--state DIR] [--port N]
```

Source: [serve.rs](../../crates/daemon/src/serve.rs). The dispatcher calls it with the solver of `fh validate`.

The command runs the daemon of the [run contract](../run-contract.md) on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP.
`--state` defaults to `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy` (`%LOCALAPPDATA%\femahoy` on Windows).
`--port` defaults to 0, a free port. The solver is `FH_SOLVE` when set, otherwise `fh-solve` beside `fh`.

| Condition | Diagnostic or output | Exit |
|---|---|---:|
| Start | stdout: `fh serve: http://127.0.0.1:PORT/?token=TOKEN` and the state folder. `DIR/daemon.info` gets the port and the token. | |
| Stop signal | stdout: `fh serve: stopped`. `daemon.info` is removed. | 0 |
| The daemon cannot start | stderr: `fh serve: DIR: MESSAGE (CODE)`, for example `daemon.running` | 1 |
| Missing option value | `fh serve: OPTION needs a value` | 2 |
| Bad port | `fh serve: --port needs a number from 0 to 65535` | 2 |
| Any other argument | `fh serve: unknown option: ARGUMENT` | 2 |

## Keep this list current

Run `fh --help` from the branch under review after a CLI change.
Compare dispatch and exit codes with `main.rs` and every command’s `run` function.
Check refusal identifiers in both STEP readers. `mesh` uses `unit.step.mixed`, while `info` uses `unit.step.conflict`.
Check returned errors as well as explicit exit codes. The dispatcher converts returned errors to exit code 2.
