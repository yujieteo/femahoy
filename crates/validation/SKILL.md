# validation skill

## Purpose

Load case TOML files, measure solver results and compare them with declared references.

## Inputs and outputs

- Each case resides in `validation/cases/<id>/case.toml`. The accepted schema is `schema = 3`. Each case has 1 mesh.
- The top-level `units` key is required. Values are `SI-mm` or `SI-m`.
- SI-mm uses mm, N, MPa, tonne and s. SI-m uses m, N, Pa, kg and s.
- Each reference names a quantity with `set`, `field`, `component`, `step` and `op`.
- Fields are `U`, `RF` and `S`. Operations are `value`, `mean`, `max`, `min` and `sum`.

## Invariants the code must keep

- All case numbers use the declared unit system. Units are not inferred from the mesh.
- Each quantity passes if `|computed - ref| <= rel_tol * |ref| + abs_tol`.
- A failed case or unavailable solver makes `fh validate` fail.
- Fixtures are data files, not prose (DESIGN.md section 10.4).
- A failure blocks the merge. There is no override (DESIGN.md section 10.5).

## Forbidden actions

- Do not describe `no solver` as a successful validation result.
- Run Abaqus. No Abaqus licence is available (DESIGN.md section 10.1).

## Validation hooks it must pass

Run `cargo check -p fh-validation` and `cargo test -p fh-validation`. Use `fh validate` for the suite.

## Examples

```sh
fh validate --root .
fh info part.step --case validation/cases/cantilever-c3d10
```
See `SCHEMA.md` for the full fixture structure.
