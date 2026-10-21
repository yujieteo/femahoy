# Check file fields

A validation case is a folder `validation/cases/<id>/` with these files that `fh validate` reads:

- `job.inp`: a plain CalculiX input. It holds the mesh, the model, the steps and the print requests that the check needs. It starts with the line `** units: SI-mm` or `** units: SI-m`. A large mesh can be in a file that `job.inp` includes (`*INCLUDE`).
- Or, for CFD, 1 or more OpenFOAM case folders in subfolders, for example `a4.04/system/controlDict`. A case with OpenFOAM checks has no `job.inp`.
- `check.toml`: the reference values, what to measure and the tolerance.

Other files in the folder (`derivation.md`, the STEP file, scripts) are documents. No field points to them.

Sources: [check reader](../../crates/fh/src/validation/fixture.rs),
[pass rule](../../crates/fh/src/validation/run.rs),
[validation command](../../crates/fh/src/validate.rs), and
[measurements](../../crates/fh/src/validation/measure.rs).

## Fields

| Key | Meaning |
|---|---|
| `rel_tol`, `abs_tol` | Numbers, 0 or more. Required. |
| `[<name>]` | One table for each check. At least 1. |

A check of `U`, `RF`, `S` or `NT` has these 5 keys. All are required.

| Key | Rule |
|---|---|
| `value` | The reference value, a finite number in the units of `job.inp`. |
| `set` | Node set (fields `U`, `RF`, `NT`) or element set (field `S`) in a print request of `job.inp`. The match ignores letter case. |
| `field` | `U` (displacement), `RF` (reaction force) and `NT` (temperature) from `*NODE PRINT`. `S` (stress) from `*EL PRINT`. |
| `component` | `U` and `RF`: 1, 2, 3. `S`: 11, 22, 33, 12, 13, 23. `NT`: 1. |
| `op` | `value` (the set holds exactly 1 value), `mean`, `max` or `sum`. |

A check of `FREQ` or `BUCKLE` has 3 keys: `value`, `field` and `mode` (the mode number, 1 or more).
`FREQ` is the frequency in cycles per time from the eigenvalue table of `*FREQUENCY`.
`BUCKLE` is the buckling factor from the table of `*BUCKLE`.

A check of `CSTR` (contact stress, from `*CONTACT PRINT`) has 4 keys: `value`, `field`, `component` and `op`. Component 1 is the pressure, 2 and 3 are the shear stresses. It has no `set`: ccx writes the contact stress of all contact elements.

An OpenFOAM check has 4 keys: `value`, `case`, `file` and `column`. It has no `field`.

| Key | Rule |
|---|---|
| `case` | The name of the subfolder with the OpenFOAM case. |
| `file` | The path of a text table in the case folder after the solve, for example `postProcessing/forceCoeffs/0/coefficient.dat`. A function object of the case writes it. |
| `column` | The name of the column in the last line of the table that starts with `#`. The value is the number in this column of the last row. |

Each column must hold 1 number. A function object writes a vector field as `(x y z)`: use the function object `components` to make the fields `Ux`, `Uy` and `Uz`.
The column `CmPitch` of `forceCoeffs` is the moment about the point `CofR` and the axis `liftDir` x `dragDir`. Thus, in the x-y plane with the flow to +x and the lift to +y, it is positive nose up.
A file has only OpenFOAM checks, or none.

`SI-mm` uses mm, N, MPa, tonne, and s. `SI-m` uses m, N, Pa, kg, and s.
The reader rejects an unknown key, a missing key and a wrong type.

## Pass rule

Each check passes if `abs(computed - reference) <= rel_tol * abs(reference) + abs_tol`.
All numbers must be finite.

`fh validate` solves `job.inp` with `ccx`. It reads the last block of the field and the set in `job.dat`.
For OpenFOAM checks, it solves each OpenFOAM case of the file at the same time, as `fh run` does. Each case has the time limit of 300 s.
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
See [bar-pressure](../../validation/cases/bar-pressure/check.toml) for a complete case.

## Errors

An error names the file and, when available, its line: `FILE:LINE: MESSAGE` or `FILE: MESSAGE`.
