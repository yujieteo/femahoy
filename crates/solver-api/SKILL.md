# solver-api skill

## Purpose

Define the `Solver` trait, registry and subprocess protocol v2.

## Inputs and outputs

- Start a solver with `exe INPUT.inp RUN_DIR`. Add `--restart` to resume a run.
- The first stdout line must be `fh-solver-protocol 2`.
- The solver writes `results.pvd`, its listed VTU files and `summary.toon` with schema `fh.run`.
- Stdout events include `progress`, `estimate` and `restart`. Other lines are logs.

## Invariants the code must keep

- A fresh run clears only an empty folder or a folder with the `.fh-run` marker.
- Result paths must stay inside the run folder.
- Defaults are a 600-second timeout, 5-second cancel grace and 85 percent of detected RAM.
- The runner sends `cancel` through stdin and enforces limits on the process tree.
- New subprocess solvers have `validated: false`.
- Results from a solver that has not passed the validation suite are marked unvalidated (DESIGN.md section 9.2).
- Line budget: 3k lines, tests included (DESIGN.md section 11).

## Forbidden actions

- Do not accept a protocol v1 header or result paths outside the run folder.
- Mark results from an unvalidated solver as validated (DESIGN.md section 9.2).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-solver-api` and `cargo test -p fh-solver-api`. Protocol tests check subprocess results, limits, cancellation and restart.

## Examples

Use `SubprocessSolver::new(exe).run(inp, run_dir)` for a fresh run.
