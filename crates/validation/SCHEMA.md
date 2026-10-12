# Fixture schema

DESIGN.md section 10.4 leaves the schema open. This file records the choice made in M1.
The captain decided on TOML and approved the `toml_edit` crate for the reader.
The captain also accepted the cantilever reference error of 2 percent and the Lamé reference error of 0.

## Choice

- One folder for each case: `validation/cases/<id>/`.
- One data file in the folder: `case.toml`. Humans read and write it.
- The crate reads it with `toml_edit` (MIT or Apache-2.0, no `serde`). `toml_edit` gives the line of each item, so errors name a line.

## Keys

| Key | Count | Meaning |
|---|---|---|
| `id` | 1 | Case name (string). |
| `class` | 1 | `"L"`, `"N"` or `"R"`. |
| `justification` | 1 | Why the class and the tolerance are right (string). |
| `derivation` | 1 | A file in the folder that shows how the reference values were derived. |
| `mesh` | 1 | Array of input files, path relative to the folder, coarse to fine. |
| `[ref]` | 1 table, 1 or more entries | `name = value`. The reference values. Put this table last in the file. |
| `ratio`, `reference_error` | class L | Mesh size ratio, and the relative error of the reference. |
| `rel_tol`, `abs_tol` | class N | Pass if `|computed - ref| <= rel_tol * |ref| + abs_tol`. |
| `rate`, `band` | class R | Expected convergence rate, and the allowed band. |

## Example

```toml
id = "patch-test"
class = "N"
justification = "Linear field, so no discretisation error."
derivation = "derivation.md"
mesh = ["../../../fixtures/inp/patch-test.inp"]
rel_tol = 1e-8
abs_tol = 1e-12

[ref]
u9_x = 0.000325
```

## Rules

- Class L needs at least 3 `mesh` entries. Class N needs exactly 1 (the reference mesh). Class R needs at least 2.
- Class L passes if the finest value is within the Richardson error estimate plus `reference_error * |ref|`.
- An unknown key, a missing key, a wrong type or a missing file is an error. The message names the file and the line.
- No Abaqus text, files or numbers. No NAFEMS values.
