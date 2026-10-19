# Check file fields

A validation case is a folder `validation/cases/<id>/` with 2 files that `fh validate` reads:

- `job.inp`: a plain CalculiX input. It holds the mesh, the model, the steps and the print requests that the check needs. It starts with the line `** units: SI-mm` or `** units: SI-m`. A large mesh can be in a file that `job.inp` includes (`*INCLUDE`).
- `check.toml`: the reference values, what to measure and the tolerance.

Other files in the folder (`derivation.md`, the STEP file, scripts) are documents. No field points to them.

Sources: [check reader](../../crates/validation/src/fixture.rs),
[pass rule](../../crates/validation/src/run.rs),
[validation command](../../crates/cli/src/validate.rs), and
[measurements](../../crates/cli/src/measure.rs).

## Fields

| Key | Meaning |
|---|---|
| `rel_tol`, `abs_tol` | Numbers, 0 or more. Required. |
| `[<name>]` | One table for each check. At least 1. |

Each check table has these 5 keys. All are required.

| Key | Rule |
|---|---|
| `value` | The reference value, a finite number in the units of `job.inp`. |
| `set` | Node set (fields `U`, `RF`) or element set (field `S`) in a print request of `job.inp`. The match ignores letter case. |
| `field` | `U` (displacement) and `RF` (reaction force) from `*NODE PRINT`. `S` (stress) from `*EL PRINT`. |
| `component` | `U` and `RF`: 1, 2, 3. `S`: 11, 22, 33, 12, 13, 23. |
| `op` | `value` (the set holds exactly 1 value), `mean`, `max`, `min` or `sum`. |

`SI-mm` uses mm, N, MPa, tonne, and s. `SI-m` uses m, N, Pa, kg, and s.
The reader rejects an unknown key, a missing key and a wrong type.

## Pass rule

Each check passes if `abs(computed - reference) <= rel_tol * abs(reference) + abs_tol`.
All numbers must be finite.

`fh validate` solves `job.inp` with `ccx`. It reads the last block of the field and the set in `job.dat`.
The stress is the value at each integration point of each element of the set.
A check with no print request for its set is an error.

## Example

```toml
# Linear field, so C3D10 has no discretisation error.
rel_tol = 1e-6
abs_tol = 1e-6

[rf_x]
value = -42000.0
set = "FIXED"
field = "RF"
component = 1
op = "sum"
```

`job.inp` holds `*NODE PRINT, NSET=FIXED` and `RF` in its step.
See [bar-tension](../../validation/cases/bar-tension/check.toml) for a complete case.

## Errors

An error names the file and, when available, its line: `FILE:LINE: MESSAGE` or `FILE: MESSAGE`.
See the [command reference](fh.md) for exit codes.
