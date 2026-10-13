# cli skill

## Purpose

The `fh` binary checks models, runs external gmsh, checks units and meshes, and runs validation cases.

## Inputs and outputs

- `fh check [--root DIR] [FILE.inp...]` prints text counts and errors, not TOON.
- Without file arguments, `check` reads `.inp` files under `fixtures/` and `validation/cases/`.
- `fh mesh FILE.(geo|step) [-o OUT.msh] [--order 1|2]` writes MSH 4.1 ASCII.
- Mesh options include `--size H`, `--scale S`, repeated `--set NAME=VALUE`, `--units SI-mm|SI-m`, `--timeout SECONDS`, and `--gmsh PATH`.
- Mesh defaults are order 2, SI-mm, 300 seconds and the input path with a `.msh` extension.
- `fh info FILE.step|FILE.msh [--case DIR]` also accepts `.stp`. It prints text.
- STEP checks report units. With `--case`, they refuse a mismatch against `case.toml`.
- MSH checks report counts, bounds, volume and minimum scaled Jacobian. They refuse empty solid meshes and nonpositive corner Jacobians.
- `fh validate [--root DIR] [--convergence PATH]` uses `FH_SOLVE` or the adjacent `fh-solve` binary.

## Invariants the code must keep

- `.geo` checks refuse `SystemCall`, `NonBlockingSystemCall`, `OnelabRun`, `Include`, and `Merge` without a plain `.msh` filename.
- STEP mesh checks refuse missing, mixed or mismatched length units.
- gmsh must be at least 4.8. The tested release line is 4.15.
- Mesh runs fail on gmsh errors, timeout, unreadable output or output without supported volume elements.
- `results`, `plot`, `serve`, `init`, `team` and `skills` are not implemented.

## Forbidden actions

Do not describe planned commands as available. Do not imply that `mesh` checks Jacobian quality. Use `info` for that check.

## Validation hooks it must pass

Run `cargo check -p fh-cli` and `cargo test -p fh-cli`. `fh --help` lists available commands.

## Examples

```sh
fh check --root .
fh mesh part.step --units SI-mm --order 2 -o part.msh
fh info part.step --case validation/cases/cantilever
fh info part.msh --case validation/cases/cantilever
fh validate --root .
```
