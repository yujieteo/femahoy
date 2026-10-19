# solver-ccx skill

## Purpose

Run CalculiX (`ccx`) as the solver of femahoy (D-13, D-31). The crate is a library: `fh serve` and the `fh` command call it in their own process, and it starts `ccx` directly. User docs: `docs/ccx.md`.

## Inputs and outputs

- `Ccx::new().run(INPUT.inp, RUN_DIR)`, or `run_with` and a `CancelToken`. `ccx` is `$FH_CCX`, else `ccx` on PATH.
- The adapter copies the input unchanged to `RUN_DIR/ccx/job.inp`, with each `*INCLUDE` file at its relative path. It runs `ccx -i job` there and reads `job.frd`.
- Every nodal field of `job.frd` goes to the VTU files. `job.dat` and `job.sta` stay in `RUN_DIR/ccx` for the person.
- It writes `step-N.vtu` and `results.pvd` in `RUN_DIR`. There is no summary file.

## Invariants the code must keep

- Start `ccx` as a separate program only. Never link, bundle or copy `ccx` code. It is GPL.
- Write each result file under a temporary name and rename it (D-09).
- `ccx` runs in its own process group. A cancel, the time limit and the memory limit kill the whole group. A guard process in the group kills it when `fh` stops.
- The default memory limit is 85 % of the RAM. There is no default time limit.
- Never change the input. `ccx` checks every keyword.

## Forbidden actions

The rules of `AGENTS.md` section 4 apply. This crate adds none.

## Validation hooks it must pass

`cargo test -p fh-solver-ccx`. The fake `ccx` tests always run. The real `ccx` tests skip with a message when `ccx` is absent. With `FH_REQUIRE_CCX=1` (CI), they fail.
With `ccx` installed: `fh validate`.

## Examples

`FH_CCX=/path/to/ccx fh run validation/cases/cantilever-c3d10/mesh-8.inp`
