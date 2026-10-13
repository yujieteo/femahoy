# Workflow lock: the locked interface

This page fixes the commands, options, file formats and exit codes of the workflow from a build123d part to validated results.
The M3 work uses these interfaces. A change to a locked item needs a decision from the captain.
`docs/reference/fh.md` has the full diagnostic tables. `docs/cad-recipe.md` has the CAD steps.

## The proof

Run `scripts/check-workflow.sh` from the repository root. It runs the case `validation/cases/bar-tension` and prints a pass or fail table.
The script needs `cargo`, gmsh and a Python with build123d. Set `PYTHON` and `FH_GMSH` to use other programs.
The script exits with 0 when each step passes and with 1 when a step fails.

| Step | Program | Result |
|---|---|---|
| `cad-step` | `scripts/cad/bar.py` (build123d) | `bar.step` in mm |
| `step-units` | `fh info bar.step --case DIR` | The STEP unit matches the case |
| `mesh` | `fh mesh bar.step --case DIR --size 8 -o bar.msh` | `C3D10` mesh and `bar.msh.units` |
| `mesh-quality` | `fh info bar.msh --case DIR` | Positive Jacobian, volume 2.0e4 mm3 |
| `model` | `fh model bar.msh --case DIR -o bar.inp` | Solver input |
| `check-inp` | `fh check bar.inp` | The parser accepts the input |
| `refuse-unit-mismatch` | `fh info bar.step` with a case in `SI-m` | Exit 1 and `unit.step.mismatch` |
| `solve-validate` | `fh validate --root ROOT` | The solver runs and the values match the reference |

Measured on this machine (macOS arm64, gmsh 4.15.2, Python 3.12, build123d 0.13.0):

| Quantity | Computed | Analytical reference | Tolerance |
|---|---:|---:|---:|
| `s_xx` (mean stress, MPa) | 2.1000e2 | 2.1e2 (E x strain) | 2.11e-4 |
| `rf_x` (sum of reactions, N) | -4.2000e4 | -4.2e4 (stress x area) | 4.2e-2 |

## Commands and options

| Command | Locked form |
|---|---|
| Build the part | `python SCRIPT.py OUT.step`. The script is the user's. `fh` ships no CAD code (D-17). |
| Check units | `fh info FILE.step\|FILE.msh [--case DIR]` |
| Mesh | `fh mesh FILE.(geo\|step) (--case DIR \| --units SI-mm\|SI-m) [-o OUT.msh] [--order 1\|2] [--size H] [--scale S] [--set NAME=VALUE]... [--timeout SECONDS] [--gmsh PATH]` |
| Model | `fh model FILE.msh --case DIR [-o OUT.inp] [--mesh-units SI-mm\|SI-m]` |
| Check input | `fh check [--root DIR] [FILE.inp...]` |
| Solve | `fh-solve INPUT.inp RUN_DIR [--restart] [--formats text,vtu,vtk] [--restart-every N] [--restart-minutes T]` |
| Validate | `fh validate [--root DIR] [--convergence PATH]` |

`fh mesh` takes the unit from `--case` or from `--units`. It has no default unit (D-18).
`fh validate` finds `fh-solve` through `FH_SOLVE`, or beside `fh`. `fh mesh` finds gmsh through `--gmsh`, `FH_GMSH`, then `PATH`.
`fh-solve` reads stdin. End of stdin means cancel, so keep stdin open (`docs/solver-protocol.md`).

## File formats

| File | Format |
|---|---|
| `case.toml` | Schema 2 (`docs/reference/case-toml.md`). `units` is `"SI-mm"` or `"SI-m"` and has no default. The `[model]` table holds the step, the material, the node sets (`axis`, `at`), the boundaries and the `[[model.cload]]` loads. |
| `*.step` | STEP with one length unit record. `fh` reads the record as text. |
| `*.msh` | MSH 4.1 ASCII, complete quadratic tetrahedra (`C3D10`). Hex8 only for an extruded part. |
| `*.msh.units` | One line, `SI-mm` or `SI-m`. `fh mesh` writes it. `fh model` reads it. |
| `*.inp` | The keyword subset in `docs/reference/inp.md`. |
| Run folder | `results.pvd`, `step-N.vtu`, optional `step-N.txt` and `.vtk`, and `summary.toon` (`schema: fh.run`). TOON numbers have 4 significant figures. The VTU files have full precision. |

## Exit codes

| Program | 0 | 1 | 2 | 130 |
|---|---|---|---|---|
| `fh mesh`, `fh info`, `fh model` | Success | A refusal or a failure of an external program | Usage error, or a case that cannot load (`fh mesh` returns 1 for this) | Not used |
| `fh check`, `fh validate` | All pass | A file or a case fails | Usage error, or no input found | Not used |
| `fh-solve` | Success | Solver failure | Usage error | Cancelled |

Refusal codes in the path: `unit.step.missing`, `unit.step.conflict`, `unit.step.mixed`, `unit.step.mismatch`,
`unit.mesh.missing`, `unit.mesh.unreadable`, `unit.mesh.conflict`, `unit.mesh.mismatch`, `mesh.empty`, `mesh.mixed`,
`mesh.jacobian.negative`, `model.missing`, `model.inp.invalid`.

## Open lock items

The lock is not complete. These items stay open in other tasks:

- Manufactured-solution cases (T7).
- Kirsch plate (T6).
- Named regions, and `*DSLOAD` on a named surface.
- Units in the results. `summary.toon` and the PVD file report `unspecified`.
- The hex8 rule in the run command.
- The two-size check and the metre-millimetre equivalence check.
- Known defect: every `fh` command loads the case and needs each `mesh` file to exist. The proof script uses a placeholder `.inp` until `fh model` writes the real one.
