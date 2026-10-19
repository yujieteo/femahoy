# Check file

DESIGN.md section 10.4 leaves the schema open. This file records the choice.
The captain decided on TOML and approved the `toml_edit` crate for the reader.
The captain also accepted the cantilever reference error of 2 percent and the Lamé reference error of 0.

## Choice

- One folder for each case: `validation/cases/<id>/`.
- `job.inp`: a plain CalculiX input. It holds the mesh, the model, the step and the print requests that the check needs. It starts with the line `** units: SI-mm` or `** units: SI-m`. Large meshes can be in a file that `job.inp` includes (`*INCLUDE`).
- `check.toml`: only the reference values, what to measure and the tolerance. Humans read and write it.
- Other files (`derivation.md`, the STEP file, scripts) are documents. No field points to them.
- The crate reads `check.toml` with `toml_edit` (MIT or Apache-2.0, no `serde`). `toml_edit` gives the line of each item, so errors name a line.

## Keys

| Key | Count | Meaning |
|---|---|---|
| `rel_tol`, `abs_tol` | 1 each | Pass if `|computed - reference| <= rel_tol * |reference| + abs_tol`. Both tolerances are 0 or more. |
| `[<name>]` | 1 or more tables | One check. See below. |

## Check table

| Key | Meaning |
|---|---|
| `value` | The reference value, in the units of `job.inp`. |
| `set` | Name of the node set (fields `U`, `RF`) or element set (field `S`) in a print request of `job.inp`. The match ignores case. |
| `field` | `"U"` displacement and `"RF"` reaction force, from `*NODE PRINT`. `"S"` stress, from `*EL PRINT`. |
| `component` | Integer. `U` and `RF`: 1, 2 or 3. `S`: 11, 22, 33, 12, 13 or 23. |
| `op` | `"value"` (the set holds one value), `"mean"`, `"max"`, `"min"` or `"sum"`. |

`fh validate` runs `ccx` and reads the last block of the field and set in `job.dat`. Stress is the value at each integration point of each element of the set.
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

## Rules

- Every number must be finite. An infinite or NaN value in a check file, a result or a tolerance is an error and never a pass.
- An unknown key, a missing key, a wrong type or a missing file is an error. The message names the file and the line.
- No Abaqus text, files or numbers. No NAFEMS values.
