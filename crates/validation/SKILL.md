# validation skill

## Purpose

Load the check files of the cases, and compare the numbers of a solver with the references.

## Inputs and outputs

- Each case resides in `validation/cases/<id>/`. It holds `job.inp` (a plain CalculiX input with its own `*NODE PRINT` and `*EL PRINT` requests) and `check.toml`.
- `check.toml` holds `rel_tol`, `abs_tol` and 1 table for each reference. A table has `value`, `set`, `field`, `component` and `op`.
- Fields are `U`, `RF` and `S`. Operations are `value`, `mean`, `max`, `min` and `sum`.
- The value comes from the last block of the field and set in the `.dat` file.

## Invariants the code must keep

- All case numbers use the unit system of the `** units:` line of `job.inp`.
- Each quantity passes if `|computed - ref| <= rel_tol * |ref| + abs_tol`.
- A failed case or unavailable solver makes `fh validate` fail.
- Check files are data files, not prose (DESIGN.md section 10.4).
- A failure blocks the merge. There is no override (DESIGN.md section 10.5).

## Forbidden actions

- Do not describe `no solver` as a successful validation result.
- Run Abaqus. No Abaqus licence is available (DESIGN.md section 10.1).

## Validation hooks it must pass

Run `cargo check -p fh-validation` and `cargo test -p fh-validation`. Use `fh validate` for the suite.

## Examples

```sh
fh validate --root .
```
See `SCHEMA.md` for the full structure of `check.toml`.
