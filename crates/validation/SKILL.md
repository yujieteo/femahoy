# validation skill

## Purpose

Load case TOML files, measure solver results and compare them with declared references.

## Inputs and outputs

- Each case resides in `validation/cases/<id>/case.toml`. The accepted schema is `schema = 2`.
- The top-level `units` key is required. Values are `SI-mm` or `SI-m`.
- SI-mm uses mm, N, MPa, tonne and s. SI-m uses m, N, Pa, kg and s.
- Each reference names a quantity with `set`, `field`, `component`, `step` and `op`.
- Fields are `U`, `RF` and `S`. Operations are `value`, `mean`, `max`, `min` and `sum`.

## Invariants the code must keep

- All case numbers use the declared unit system. Units are not inferred from the mesh.
- Class L uses Richardson extrapolation and observed-order bounds. Class N uses fixed relative and absolute tolerances.
- Class R checks a rate within a declared band.
- Convergence over a case collection skips class N. A direct class N convergence request fails.
- A failed case or unavailable solver makes `fh validate` fail.

## Forbidden actions

Do not describe `no solver` as a successful validation result.

## Validation hooks it must pass

Run `cargo check -p fh-validation` and `cargo test -p fh-validation`. Use `fh validate` for the suite.

## Examples

```sh
fh validate --root .
fh validate --convergence validation/cases/cantilever
fh info part.step --case validation/cases/cantilever
```
See `SCHEMA.md` for the full fixture structure.
