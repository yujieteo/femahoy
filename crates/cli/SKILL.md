# cli skill

## Purpose

The `fh` binary runs external gmsh, checks units and meshes, runs validation cases, and solves models on the daemon (`fh run`).

## Inputs and outputs

- `fh mesh FILE.(geo|step) [-o OUT.msh] [--order 1|2]` writes MSH 4.1 ASCII.
- Mesh options include `--size H`, `--scale S`, repeated `--set NAME=VALUE`, `--units SI-mm|SI-m`, `--timeout SECONDS`, and `--gmsh PATH`.
- Mesh defaults are order 2, SI-mm, no time limit and the input path with a `.msh` extension.
- `fh model FILE.msh --case DIR [-o OUT.inp] [--mesh-units SI-mm|SI-m]` writes a solver input from the mesh and the `[model]` table of `case.toml`. It refuses a missing or different unit record (`unit.mesh.*`), an empty set, a volume with no physical group (`mesh.volume.unnamed`) and a region that the mesh does not hold (`mesh.region.missing`, `mesh.region.inner`). `fh mesh` writes the unit record `OUT.msh.units`.
- `fh model` writes each `[[model.region]]` as `*NSET` and `*SURFACE, TYPE=ELEMENT` of the same name, each `[[model.dsload]]` as `*DSLOAD`, and the line `** units: SI-mm` (or `SI-m`) before the first keyword. `fh-solve-ccx` copies those units into the result files.
- `fh mesh FILE.step --case DIR` puts every volume in the physical group `SOLID` and makes a physical surface for each region of the case (the fixed script contract of M2b). It refuses a region that selects no face (`mesh.region.missing`) and a volume with no physical group.
- `fh info`, `fh mesh` and `fh model` load the case without its `mesh` files (`Case::load_before_mesh`), because the workflow makes them later.
- `fh info FILE.step|FILE.msh [--case DIR]` also accepts `.stp`. It prints text.
- STEP checks report units. With `--case`, they refuse a mismatch against `case.toml`.
- MSH checks report counts, bounds, volume and minimum scaled Jacobian. They refuse empty solid meshes and nonpositive corner Jacobians.
- `fh validate [--root DIR]` uses `FH_SOLVE`, else `fh-solve-ccx` beside `fh` or on `PATH`. It stops when `ccx` does not start, and fails when no case ran.
- `fh serve [--state DIR] [--port N] [--no-open]` and `fh serve --stop [--state DIR]` call `fh_daemon::serve`. The daemon runs on 127.0.0.1 until SIGINT, SIGTERM or SIGHUP (`docs/run-contract.md`). It uses the same solver as `validate`.
- `fh run MODEL.inp|CASE_DIR|PART.step` and `fh run --example NAME` (`src/run.rs`) solve on the daemon of the state folder through `fh_daemon::Client`, and start `fh serve --no-open` in the background when no daemon answers. A case folder goes through `mesh::run` and `model::run` in `STATE/cases/NAME-HASH`. The example files come from `validation/cases/bar-pressure` (`include_bytes!`). Exit codes: 0, 1 (failed or refused), 2 (usage or input), 3 (no daemon), 130 (cancelled), in `docs/reference/fh.md`.

## Invariants the code must keep

- `.geo` checks refuse `SystemCall`, `NonBlockingSystemCall`, `OnelabRun`, `Include`, and `Merge` without a plain `.msh` filename.
- STEP mesh checks refuse missing, mixed or mismatched length units.
- gmsh must be at least 4.8. The tested release line is 4.15.
- Mesh runs fail on gmsh errors, timeout, unreadable output or output without supported volume elements.
- `fh serve` has no option for the listen address. The daemon binds to 127.0.0.1 only (DESIGN.md section 4.1).
- The binary is `fh`. Never `fea` (CLAUDE.md).
- Each command works over SSH and tmux, with no GUI (DESIGN.md section 3).
- `fh run` checks the arguments and the input before it starts a daemon. It never changes a case folder of the user.
- A test never opens the browser of the machine. Tests set `FH_NO_OPEN=1` and a fake `BROWSER`, and use a scratch state folder.

## Forbidden actions

- Do not describe planned commands as available. Do not imply that `mesh` checks Jacobian quality. Use `info` for that check.
- Name the binary `fea`.
- Require a GUI for any command (DESIGN.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-cli` and `cargo test -p fh-cli`. `fh --help` lists available commands. `tests/run.rs` has a test for each exit code of `fh run`. `tests/install.rs` tests `scripts/install.sh` with fake tools.

## Examples

```sh
fh mesh part.step --units SI-mm --order 2 -o part.msh
fh model part.msh --case validation/cases/bar-tension -o part.inp
fh info part.step --case validation/cases/cantilever-c3d10
fh info part.msh --case validation/cases/cantilever-c3d10
fh validate --root .
fh run --example bar-pressure
fh run validation/cases/bar-pressure --no-open
```
