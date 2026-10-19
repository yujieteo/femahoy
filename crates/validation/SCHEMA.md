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
| `schema` | 1 | The schema version. It is `3`. Any other value is an error. |
| `id` | 1 | Case name (string). |
| `units` | 1 | The unit system of every number in the case: `"SI-mm"` (mm, N, MPa, tonne, s) or `"SI-m"` (m, N, Pa, kg, s). No default. A missing or other value is an error (D-12). |
| `justification` | 1 | Why the tolerance is right (string). |
| `derivation` | 1 | A file in the folder that shows how the reference values were derived. |
| `mesh` | 1 | One input file (string), path relative to the folder. It holds only `C3D10` elements (D-19). |
| `time_limit_s` | 0 or 1 | The time limit of each solve in `fh validate`, in seconds (above 0). The default is no time limit. |
| `memory_limit_mib` | 0 or 1 | The memory limit of each solve in `fh validate`, in MiB (above 0). The default is the limit of the runner, 85 % of the RAM. |
| `[ref]` | 1 table, 1 or more entries | `name = value`. The reference values. |
| `[quantity.<name>]` | 1 table for each `[ref]` entry | What to measure for that reference. See below. |
| `rel_tol`, `abs_tol` | 1 each | Pass if `|computed - ref| <= rel_tol * |ref| + abs_tol`. Both tolerances are 0 or more. |

## Model table (optional)

`fh model` builds a solver input from a `.msh` file and the `[model]` table. The table is optional.
`fh mesh` and `fh model` load the case with `Case::load_before_mesh`, which does not need the `mesh` files.
`fh validate` uses `Case::load`, which needs them.

| Key | Meaning |
|---|---|
| `model.step` | Step name. Default `LOAD`. |
| `model.material` | `name`, `youngs_modulus`, `poissons_ratio` (isotropic, in the case units). Optional `expansion` (1/K) and `box` (6 coordinates). Use `[[model.material]]` for several materials, each with a volume box. |
| `[[model.set]]` | `name`, `axis` (`x`, `y` or `z`), `at`, optional `tol`. Alternatively, `name` and `point = [x, y, z]` select an existing node. The set holds the nodes within `tol` of that plane. The default `tol` is 1e-6 of the largest bounding box side. `ALL` is reserved. |
| `[[model.region]]` | `name`, then `axis` (`x`, `y` or `z`) and `at`, or `box = [x0, y0, z0, x1, y1, z1]`. A named group of faces of the part: the faces on the plane, or the faces fully inside the box. `fh mesh` makes a gmsh physical surface of the name. `fh model` writes a node set and an element-face surface of the name. The name starts with a letter and holds only letters, digits and `_`. Names are unique among sets and regions. `ALL` and `SOLID` are reserved. |
| `[[model.boundary]]` | `set` (a set or a region), `first_dof`, optional `last_dof`, optional `value` (default 0). At least 1 is needed. |
| `[[model.cload]]` | `set` (a set or a region), `dof`, `per_node`. The force acts on each node of the set. It is not a total force. |
| `[[model.dsload]]` | `surface` (a region), `pressure`. A uniform pressure on each face of the region (`*DSLOAD`). Positive pressure pushes into the body. |

## Measured quantity

Each `[ref]` entry has a `[quantity.<name>]` table with the same name. A case needs no Rust code for its own quantities.

| Key | Meaning |
|---|---|
| `set` | Name of a node set (fields `U`, `RF`) or an element set (field `S`) in the mesh. |
| `field` | `"U"` displacement, `"RF"` reaction force, `"S"` stress. |
| `component` | Integer. `U` and `RF`: 1, 2 or 3. `S`: 11, 22, 33, 12, 23 or 13. |
| `step` | Name of the step in the mesh. The result is the one at the end of that step. |
| `op` | `"value"` (the set holds one entity), `"mean"`, `"max"`, `"min"` or `"sum"`. |

`fh validate` checks that the mesh holds the set and the step. A quantity without a `[ref]` entry is an error. A `[ref]` entry without a quantity is an error.

`model.temperature_change` sets a uniform temperature change from 0. Each thermal material needs `expansion`.
Material boxes select physical volume groups. Each element needs exactly one material.
The full rules are in [case-toml.md](../../docs/reference/case-toml.md).

## Example

```toml
schema = 3
id = "bar-tension"
units = "SI-mm"
justification = "Linear field, so C3D10 has no discretisation error."
derivation = "derivation.md"
mesh = "bar.inp"
rel_tol = 1e-6
abs_tol = 1e-6

[ref]
rf_x = -42000.0

[quantity.rf_x]
set = "FIXED"
field = "RF"
component = 1
step = "LOAD"
op = "sum"
```

## Rules

- Every number must be finite. An infinite or NaN value in a fixture, a result or a tolerance is an error and never a pass.
- An unknown key, a missing key, a wrong type or a missing file is an error. The message names the file and the line.
- No Abaqus text, files or numbers. No NAFEMS values.
