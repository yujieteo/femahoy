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
| `units` | 1 | The unit system of every number in the case: `"SI-mm"` (mm, N, MPa, tonne, s) or `"SI-m"` (m, N, Pa, kg, s). No default. A missing or other value is an error (D-12). |
| `class` | 1 | `"L"`, `"N"` or `"R"`. |
| `justification` | 1 | Why the class and the tolerance are right (string). |
| `derivation` | 1 | A file in the folder that shows how the reference values were derived. |
| `element_types` | 1 | Array of the element types of the meshes (`C3D8`, `C3D8I`, `C3D10`, `C3D10M`). The case runs once for each type. `fh validate` fails if a mesh holds another type. |
| `solver` | 0 or 1 | The solver program: `"fh-solve"` (the default) or `"fh-solve-ccx"` (CalculiX). `fh validate` runs the case with this program. If `ccx` does not start, `fh validate` skips an `fh-solve-ccx` case. |
| `mesh` | 1 | Array of input files, path relative to the folder, coarse to fine. |
| `[ref]` | class L and N: 1 table, 1 or more entries. Not in class R. | `name = value`. The reference values. |
| `[quantity.<name>]` | class L and N: 1 table for each `[ref]` entry. Not in class R. | What to measure for that reference. See below. |
| `ratio`, `reference_error` | class L | Mesh size ratio (above 1), and the relative error of the reference (0 or more). |
| `order_min`, `order_max` | class L | The band of the observed Richardson order. Both are above 0. |
| `rel_tol`, `abs_tol` | class N | Pass if `|computed - ref| <= rel_tol * |ref| + abs_tol`. Both tolerances are 0 or more. |
| `rate`, `band` | class R | Expected convergence rate, and the allowed band (0 or more). |
| `mesh_size` | class R | Array of positive numbers, one for each `mesh`, falling from coarse to fine. |
| `boundary_set` | class R | The node set that gets the boundary values. Every mesh holds it. |
| `[exact]` | class R | Table with `u1`, `u2`, `u3`: the exact displacement as text in `x`, `y`, `z`. |

## Model table (optional)

`fh model` builds a solver input from a `.msh` file and the `[model]` table. The table is optional.
`fh info`, `fh mesh` and `fh model` load the case with `Case::load_before_mesh`, which does not need the `mesh` files.
`fh check` and `fh validate` use `Case::load`, which needs them.

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
| `set` | Name of a node set (fields `U`, `RF`) or an element set (field `S`) in every mesh. |
| `field` | `"U"` displacement, `"RF"` reaction force, `"S"` stress. |
| `component` | Integer. `U` and `RF`: 1, 2 or 3. `S`: 11, 22, 33, 12, 23 or 13. |
| `step` | Name of the step in every mesh. The result is the one at the end of that step. |
| `op` | `"value"` (the set holds one entity), `"mean"`, `"max"`, `"min"` or `"sum"`. |

`fh validate` checks that every mesh holds the set and the step. A quantity without a `[ref]` entry is an error. A `[ref]` entry without a quantity is an error.

`model.temperature_change` sets a uniform temperature change from 0. Each thermal material needs `expansion`.
Material boxes select physical volume groups. Each element needs exactly one material.
The full rules are in [case-toml.md](../../docs/reference/case-toml.md).

## Example

```toml
schema = 2
id = "patch-test"
units = "SI-mm"
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

- Class R fixes the `boundary_set` nodes to the exact field and compares `U` with the exact field at every node (root mean square). The observed rate is the least squares slope of `log(error)` against `log(mesh_size)` over all meshes. It must lie in `rate +/- band`, and the error must fall from each mesh to the next. The mesh files hold no step. The harness adds one.
- The text of `[exact]` accepts numbers, `x`, `y`, `z`, `pi`, `+ - * / ^`, brackets, and `sin`, `cos`, `sinh`, `cosh`, `exp`, `sqrt`.
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
