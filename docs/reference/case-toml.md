# Case TOML fields

`Case::load(DIR)` reads `DIR/case.toml`. The reader accepts schema version `2`.
All fields below are required for their applicable class. There are no defaults for these fields.

Sources: [fixture reader](../../crates/validation/src/fixture.rs),
[tolerance rules](../../crates/validation/src/tolerance.rs),
[CLI validation checks](../../crates/cli/src/validate.rs), and
[result measurements](../../crates/cli/src/measure.rs).

## Common fields

| Field | Type | Accepted value or rule |
|---|---|---|
| `schema` | Integer | Exactly `2`. |
| `id` | Non-empty string | The case name. |
| `units` | Non-empty string | Exactly `SI-mm` or `SI-m`. |
| `class` | Non-empty string | Exactly `L`, `N`, or `R`. |
| `justification` | Non-empty string | The reason for the class and tolerances. |
| `derivation` | Non-empty string | Path to an existing file with the reference derivation. |
| `element_types` | Non-empty array of strings | Each entry is `C3D8`, `C3D8I`, `C3D10`, or `C3D10M`. Parser acceptance does not establish solver support. |
| `mesh` | Array of strings | Paths to existing input files, coarse to fine. Class L needs at least 3. N needs exactly 1. R needs at least 2. |
| `[ref]` | Non-empty table | Each entry maps a reference name to a finite number. |
| `[quantity.<name>]` | Table for each reference name | Defines the measurement for the matching entry in `[ref]`. |

Relative `derivation` and `mesh` paths start at the case directory. Absolute paths remain absolute.
`SI-mm` uses mm, N, MPa, tonne, and s. `SI-m` uses m, N, Pa, kg, and s.
The reader rejects unknown top-level keys and class fields that belong to another class.

## Class fields

Numbers can be TOML integers or floats. All numbers must be finite.

| Class | Field | Rule |
|---|---|---|
| L | `ratio` | Mesh size ratio above 1. |
| L | `reference_error` | Relative reference error, 0 or more. |
| L | `order_min` | Observed order bound above 0. Must be below `order_max`. |
| L | `order_max` | Observed order bound above 0. |
| N | `rel_tol` | Relative tolerance, 0 or more. |
| N | `abs_tol` | Absolute tolerance, 0 or more. |
| R | `rate` | Expected convergence rate. Any finite number. |
| R | `band` | Allowed difference from the expected rate, 0 or more. |
| R | `mesh_size` | Array of positive numbers, one for each mesh, falling from coarse to fine. |
| R | `boundary_set` | Name of the node set that gets the boundary values. Each mesh must hold it. |
| R | `[exact]` | Table with `u1`, `u2` and `u3`: the exact displacement as text, see below. |

Class L uses the last 3 mesh values for Richardson extrapolation.
Its observed order must lie in the inclusive interval from `order_min` to `order_max`.
Its comparison is `abs(limit - reference) <= estimate + reference_error * abs(reference)`.
Class N uses `abs(computed - reference) <= rel_tol * abs(reference) + abs_tol`.
The `check_r` function compares the slope of `log(error)` against `log(h)` with `rate`, within `band`.

A class R case has no `[ref]` table and no `[quantity]` table. It needs at least 2 meshes.
The meshes hold no step. The harness adds one step with a `*BOUNDARY` value for each node of `boundary_set`,
taken from the exact field. The error of a mesh is the root mean square, over all nodes, of the distance between
the computed displacement and the exact field. The observed rate is the slope of `log(error)` against `log(mesh_size)`.

The exact field uses `x`, `y`, `z`, `pi`, numbers, `+ - * / ^`, brackets, and the functions
`sin`, `cos`, `sinh`, `cosh`, `exp` and `sqrt`. The field must solve the elasticity equations with no body force.

## Quantity fields

Each quantity table permits exactly these 5 fields. All 5 are required.

| Field | Type | Accepted value or rule |
|---|---|---|
| `set` | Non-empty string | Node set for `U` or `RF`. Element set for `S`. |
| `field` | Non-empty string | `U` (displacement), `RF` (reaction force), or `S` (stress). |
| `component` | Integer | `U` and `RF`: 1, 2, 3. `S`: 11, 22, 33, 12, 23, 13. |
| `step` | Non-empty string | Step name. The measurement uses the result at the end of this step. |
| `op` | Non-empty string | `value`, `mean`, `max`, `min`, or `sum`. |

A reference without a quantity fails. A quantity without a reference fails.
The reader rejects unknown keys inside a quantity table.
The measurement refuses an empty set. `value` requires exactly 1 entity.
`fh validate` checks mesh element types, sets, and step names before the normal suite run.
Step and result-set lookup ignore letter case.

## Example

This example uses 1 displacement reference from the [patch-test case](../../validation/cases/patch-test/case.toml).
Paths below are relative to that case directory.

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

## Model and thermal materials

The optional `[model]` table supplies the inputs to `fh model`.
The existing `[model.material]` table applies one material to every solid element.
For several materials, use `[[model.material]]` for each material, with these fields:

| Field | Rule |
|---|---|
| `name` | Unique name. Start with a letter. Use letters, digits and `_`. `ALL` is reserved. |
| `youngs_modulus` | Positive number in the case units. |
| `poissons_ratio` | Number above -1 and below 0.5. |
| `expansion` | Optional linear thermal expansion coefficient in 1/K. Required with `temperature_change`. |
| `box` | Optional `[x0, y0, z0, x1, y1, z1]` volume box with positive extents. Required for each of several materials. |

`fh mesh` fragments the STEP bodies to make a shared interface.
It assigns each volume inside a material box to the physical group of that material name.
Each solid element must belong to exactly one material group.
A material group name cannot equal a node set name or a surface region name.
`fh model` writes an element set and a solid section for each material group.

`model.temperature_change` is an optional finite number in K.
The initial temperature is 0 at every node. The final temperature equals the change.
`fh model` writes `*EXPANSION`, `*INITIAL CONDITIONS, TYPE=TEMPERATURE` and `*TEMPERATURE`.
The native solver refuses thermal input. Use the CalculiX adapter.
The top-level `solver = "fh-solve-ccx"` selects the adapter for `fh validate`.
For `fh run`, set `FH_SOLVE` before the private daemon starts.

A `[[model.set]]` can use `point = [x, y, z]` instead of `axis` and `at`.
The point selector uses the default tolerance of 1e-6 of the largest mesh extent.
It refuses `axis`, `at` or `tol` with `point`.
The point must coincide with an existing mesh node. An empty selection fails.
See [warpage-strip](../../validation/cases/warpage-strip/case.toml) for a complete case.

## Errors and maintenance

Fixture errors name the file and, when available, its line: `FILE:LINE: MESSAGE` or `FILE: MESSAGE`.
Examples include `missing key`, `unknown key`, unsupported schema version, wrong type, and missing files.
See the [command reference](fh.md) for exit codes.

After a schema change, compare this list with `SCHEMA_VERSION`, `KEYS`, `build`, and `quantity` in the fixture reader.
Check the example against the actual case file. Check measurement rules against `extract` and the tolerance functions.
