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
| `schema` | 1 | The schema version. It is `2`. Any other value is an error. |
| `id` | 1 | Case name (string). |
| `class` | 1 | `"L"`, `"N"` or `"R"`. |
| `justification` | 1 | Why the class and the tolerance are right (string). |
| `derivation` | 1 | A file in the folder that shows how the reference values were derived. |
| `element_types` | 1 | Array of the element types of the meshes (`C3D8`, `C3D8I`, `C3D10`, `C3D10M`). The case runs once for each type. `fh validate` fails if a mesh holds another type. |
| `mesh` | 1 | Array of input files, path relative to the folder, coarse to fine. |
| `[ref]` | 1 table, 1 or more entries | `name = value`. The reference values. |
| `[quantity.<name>]` | 1 table for each `[ref]` entry | What to measure for that reference. See below. |
| `ratio`, `reference_error` | class L | Mesh size ratio (above 1), and the relative error of the reference (0 or more). |
| `order_min`, `order_max` | class L | The band of the observed Richardson order. Both are above 0. |
| `rel_tol`, `abs_tol` | class N | Pass if `|computed - ref| <= rel_tol * |ref| + abs_tol`. Both tolerances are 0 or more. |
| `rate`, `band` | class R | Expected convergence rate, and the allowed band (0 or more). |

## Measured quantity

Each `[ref]` entry has a `[quantity.<name>]` table with the same name. A case needs no Rust code for its own quantities.

| Key | Meaning |
|---|---|
| `set` | Name of a node set (fields `U`, `RF`) or an element set (field `S`) in every mesh. |
| `field` | `"U"` displacement, `"RF"` reaction force, `"S"` stress. |
| `component` | Integer. `U` and `RF`: 1, 2 or 3. `S`: 11, 22, 33, 12, 23 or 13. |
| `step` | Name of the step in every mesh. The result is the one at the end of that step. |
| `op` | `"value"` (the set holds one entity), `"mean"`, `"max"`, `"min"` or `"sum"`. |

`fh validate` checks that every mesh holds the set and the step. A quantity without a `[ref]` entry is an error. A `[ref]` entry without a quantity is an error.

## Example

```toml
schema = 2
id = "patch-test"
class = "N"
justification = "Linear field, so no discretisation error."
derivation = "derivation.md"
element_types = ["C3D8"]
mesh = ["../../../fixtures/inp/patch-test.inp"]
rel_tol = 1e-8
abs_tol = 1e-12

[ref]
u9_x = 0.000325

[quantity.u9_x]
set = "N9"
field = "U"
component = 1
step = "LOAD"
op = "value"
```

## Rules

- Class L needs at least 3 `mesh` entries. Class N needs exactly 1 (the reference mesh). Class R needs at least 2.
- Class L uses the last 3 mesh values for the Richardson limit, the observed order and the estimate (the error of the finest value).
- Class L passes if `|limit - ref| <= estimate + reference_error * |ref|`. The window is centred on the extrapolated limit, not on the finest value.
- Class L fails if the observed order is outside `order_min` to `order_max`. A low order widens the estimate, so the band stops it from hiding a wrong value.
- Class L rejects values whose observed Richardson order is 0 or less (no convergence). Equal mesh values are also rejected, because they have no finite observed order.
- Every number must be finite. An infinite or NaN value in a fixture, a result or a tolerance is an error and never a pass.
- `fh validate --convergence PATH` takes a case folder, or a folder of case folders. A class N case has one mesh, so it is skipped in a folder and it is an error when named alone.
- A key that belongs to another class is an error.
- An unknown key, a missing key, a wrong type or a missing file is an error. The message names the file and the line.
- No Abaqus text, files or numbers. No NAFEMS values.
