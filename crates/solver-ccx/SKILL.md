# solver-ccx skill

## Purpose

Run CalculiX (`ccx`) as a protocol v2 solver: the executable `fh-solve-ccx` (D-13). User docs: `docs/ccx.md`.

## Inputs and outputs

- `fh-solve-ccx INPUT.inp RUN_DIR [--formats vtu]`. `ccx` is `$FH_CCX`, else `ccx` on PATH.
- The adapter writes `RUN_DIR/ccx/job.inp`, runs `ccx -i job` there, and reads `job.frd` and `job.dat`.
- U and RF come from `job.frd`. The stress comes from the integration points in `job.dat`.
- It writes `step-N.vtu`, `results.pvd` and `summary.toon` in `RUN_DIR`.

## Invariants the code must keep

- Start `ccx` as a separate program only. Never link, bundle or copy `ccx` code. It is GPL.
- The summary marks the results unvalidated.
- Write each result file under a temporary name and rename it (D-09).
- A `cancel` line kills `ccx`, and the exit code is 130. The end of stdin does the same when `FH_CANCEL_ON_EOF` is `1`.
- A keyword outside the subset of `docs/ccx.md` is an error that names it.

## Forbidden actions

The rules of `AGENTS.md` section 4 apply. This crate adds none.

## Validation hooks it must pass

`cargo test -p fh-solver-ccx`. The fake `ccx` tests always run. The real `ccx` test skips with a message when `ccx` is absent.
With `ccx` installed: `FH_SOLVE=target/release/fh-solve-ccx fh validate`.

## Examples

`FH_CCX=/path/to/ccx fh-solve-ccx validation/cases/cantilever/mesh-32.inp /tmp/run`
