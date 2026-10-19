# Case TOML fields

`Case::load(DIR)` reads `DIR/case.toml`. The reader accepts schema version `3`.
All fields below are required. There are no defaults for these fields.

Sources: [fixture reader](../../crates/validation/src/fixture.rs),
[pass rule](../../crates/validation/src/run.rs),
[CLI validation checks](../../crates/cli/src/validate.rs), and
[result measurements](../../crates/cli/src/measure.rs).

## Common fields

| Field | Type | Accepted value or rule |
|---|---|---|
| `schema` | Integer | Exactly `3`. |
| `id` | Non-empty string | The case name. |
| `units` | Non-empty string | Exactly `SI-mm` or `SI-m`. |
| `justification` | Non-empty string | The reason for the tolerances. |
| `derivation` | Non-empty string | Path to an existing file with the reference derivation. |
| `mesh` | Non-empty string | Path to 1 existing input file. It holds only `C3D10` elements. |
| `[ref]` | Non-empty table | Each entry maps a reference name to a finite number. |
| `[quantity.<name>]` | Table for each reference name | Defines the measurement for the matching entry in `[ref]`. |
| `rel_tol` | Number | Relative tolerance, 0 or more. |
| `abs_tol` | Number | Absolute tolerance, 0 or more. |

Relative `derivation` and `mesh` paths start at the case directory. Absolute paths remain absolute.

These fields are optional. They set the limits of each solve of the case in `fh validate`:

| Field | Type | Accepted value or rule |
|---|---|---|
| `time_limit_s` | Number above 0 | The time limit in seconds. The default is no time limit. |
| `memory_limit_mib` | Number above 0 | The memory limit in MiB. The default is the limit of the runner, 85% of the RAM. |

At a limit, the run stops `ccx`, and the case row is `cannot run: REASON` with the limit.
`SI-mm` uses mm, N, MPa, tonne, and s. `SI-m` uses m, N, Pa, kg, and s.
The reader rejects unknown top-level keys.

## Pass rule

Numbers can be TOML integers or floats. All numbers must be finite.
Each quantity passes if `abs(computed - reference) <= rel_tol * abs(reference) + abs_tol`.

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
`fh validate` checks that the mesh parses and holds each set and step before the suite run.
Step and result-set lookup ignore letter case.

## Example

This example uses 1 reaction force reference from the [bar-tension case](../../validation/cases/bar-tension/case.toml).
The real case also has a `[model]` table and a stress reference.

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
The `ccx` run gives the thermal input to CalculiX.

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
