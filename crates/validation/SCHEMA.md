# Fixture schema (proposal for the captain)

DESIGN.md section 10.4 leaves the schema open. This file records the choice made in M1.
The captain must confirm it or ask for a change.

## Choice

- One folder for each case: `validation/cases/<id>/`.
- One data file in the folder: `case.kv`. It has lines of `key = value`. A line that starts with `#` is a comment.
- The format is not TOML or JSON. `DESIGN.md` section 11.1 names no parser crate, so the crate reads the format with its own 60-line reader.
- If the captain approves a TOML crate, the keys map to TOML one to one.

## Keys

| Key | Count | Meaning |
|---|---|---|
| `id` | 1 | Case name. |
| `class` | 1 | `L`, `N` or `R`. |
| `justification` | 1 | Why the class and the tolerance are right. One line. |
| `derivation` | 1 | A file in the folder that shows how the reference values were derived. |
| `mesh` | 1 or more | An input file, path relative to the folder, coarse to fine. |
| `ref` | 1 or more | `name value`. A reference value. |
| `ratio`, `reference_error` | class L | Mesh size ratio, and the relative error of the reference. |
| `rel_tol`, `abs_tol` | class N | Pass if `|computed - ref| <= rel_tol * |ref| + abs_tol`. |
| `rate`, `band` | class R | Expected convergence rate, and the allowed band. |

## Rules

- Class L needs at least 3 `mesh` entries. Class N needs exactly 1 (the reference mesh). Class R needs at least 2.
- Class L passes if the finest value is within the Richardson error estimate plus `reference_error * |ref|`.
- An unknown key, a missing key, a missing file or a repeated key is an error with a line number.
- No Abaqus text, files or numbers. No NAFEMS values.
