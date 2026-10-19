# cli skill

## Purpose

The `fh` binary runs external gmsh and writes the mesh as CalculiX input, runs validation cases, and solves CalculiX inputs on the daemon (`fh run`).

## Inputs and outputs

- `fh mesh FILE.(geo|step) --units SI-mm|SI-m [-o OUT.inp]` runs gmsh and writes the nodes, the `C3D10` elements (`ELSET=ALL`) and the node set `NALL` as CalculiX input text. The chat includes it with `*INCLUDE`. The first lines are `** units: SI-mm` (or `SI-m`).
- Mesh options include `--size H`, `--scale S`, repeated `--set NAME=VALUE`, `--timeout SECONDS`, and `--gmsh PATH`. The units have no default (D-18). The mesh is always order 2 (D-19). The default output is the input path with a `.inp` extension.
- Set options: `--face NAME=SEL` makes a node set and an element surface of the same name, `--nodes NAME=SEL` makes a node set, and `--volume NAME=box:...` makes an element set (STEP only). `SEL` is `x:V`, `y:V`, `z:V` or `box:X0,Y0,Z0,X1,Y1,Z1`. `--nodes` also takes `point:X,Y,Z`. It refuses an empty set (`mesh.set.empty`), a name used twice (`mesh.name.duplicate`), a face that is not on the outside of the solid, a volume with no physical group and a volume box that does not cover each element once.
- `fh validate [--root DIR]` solves each case (`job.inp`) with `ccx` (`FH_CCX`, else `ccx` on `PATH`) and compares the measures of `check.toml` with the reference values. It stops when `ccx` does not start, and fails when no case ran.
- `fh serve [--state DIR] [--port N] [--no-open]` and `fh serve --stop [--state DIR]` call `fh_daemon::serve`. The daemon runs on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP (`docs/run-contract.md`). It uses the same solver as `validate`.
- `fh run MODEL.inp [--preview] [--timeout SECONDS] [--memory MIB]` (`src/run.rs`) sends the input to the daemon of the state folder through `fh_daemon::Client`, and starts `fh serve --no-open` in the background when no daemon answers. CalculiX reads the input unchanged, with its `*INCLUDE` files. With `--preview`, the page shows the supports and the loads and no solve starts. Exit codes: 0, 1 (failed or refused), 2 (usage or input), 3 (no daemon), 130 (cancelled), in `docs/reference/fh.md`.
- `fh mesh` refuses a STEP unit that is not the declared units, and a mesh with a nonpositive corner Jacobian (`mesh.jacobian.negative`).
- `fh validate [--root DIR]` starts `ccx` (`FH_CCX`, else `ccx` on `PATH`). It stops when `ccx` does not start, and fails when no case ran.
- `fh serve [--state DIR] [--port N] [--no-open]` and `fh serve --stop [--state DIR]` call `fh_daemon::serve`. The daemon runs on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP (`docs/run-contract.md`). It uses the same solver as `validate`.
- `fh run MODEL.inp` (`src/run.rs`) solves on the daemon of the state folder through `fh_daemon::Client`, and starts `fh serve --no-open` in the background when no daemon answers. `--timeout` and `--memory` set the limits of the run. Exit codes: 0, 1 (failed or refused), 2 (usage or input), 3 (no daemon), 130 (cancelled), in `docs/reference/fh.md`.

## Invariants the code must keep

- `.geo` checks refuse `SystemCall`, `NonBlockingSystemCall`, `OnelabRun`, `Include`, and `Merge` without a plain `.msh` filename.
- STEP mesh checks refuse missing, mixed or mismatched length units.
- gmsh must be at least 4.8. The tested release line is 4.15.
- Mesh runs fail on gmsh errors, timeout, unreadable output or output without supported volume elements.
- `fh serve` has no option for the listen address. The daemon binds to 127.0.0.1 only (DESIGN.md section 4.1).
- The binary is `fh`. Never `fea` (CLAUDE.md).
- Each command works over SSH and tmux, with no GUI (DESIGN.md section 3).
- `fh run` checks the arguments and the input before it starts a daemon. It never changes the input of the user.
- A test never opens the browser of the machine. Tests set `FH_NO_OPEN=1` and a fake `BROWSER`, and use a scratch state folder.

## Forbidden actions

- Do not describe planned commands as available.
- Name the binary `fea`.
- Require a GUI for any command (DESIGN.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-cli` and `cargo test -p fh-cli`. `fh --help` lists available commands. `tests/run.rs` has a test for each exit code of `fh run`. `tests/install.rs` tests `scripts/install.sh` with fake tools.

## Examples

```sh
fh mesh part.step --units SI-mm --face FIXED=x:0 -o mesh.inp
fh validate --root .
fh run validation/cases/bar-pressure/job.inp --no-open
```
