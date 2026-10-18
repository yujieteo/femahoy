# solver-api skill

## Purpose

Define subprocess protocol v2 and its runner (`SubprocessSolver`).

## Inputs and outputs

- Start a solver with `exe INPUT.inp RUN_DIR`.
- The first stdout line must be `fh-solver-protocol 2`.
- The solver writes `results.pvd`, its listed VTU files and `summary.toon` with schema `fh.run`.
- Stdout events are `progress` and `estimate`. Other lines are logs.

## Invariants the code must keep

- A fresh run clears only an empty folder or a folder with the `.fh-run` marker.
- Result paths must stay inside the run folder.
- Defaults are no time limit, a 5-second cancel grace and 85 percent of detected RAM.
- The runner sends `cancel` through stdin and enforces limits on the process tree.
- New subprocess solvers have `validated: false`.
- Results from a solver that has not passed the validation suite are marked unvalidated (DESIGN.md section 9.2).

## Forbidden actions

- Do not accept a protocol v1 header or result paths outside the run folder.
- Mark results from an unvalidated solver as validated (DESIGN.md section 9.2).

## Validation hooks it must pass

Run `cargo check -p fh-solver-api` and `cargo test -p fh-solver-api`. Protocol tests check subprocess results, limits and cancellation.

## Examples

Use `SubprocessSolver::new(exe).run(inp, run_dir)` for a fresh run.
