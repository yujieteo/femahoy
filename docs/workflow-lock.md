# Workflow lock: the locked interface

This page fixes the commands, options, file formats and exit codes of the workflow from a build123d part to validated results.
The M3 work uses these interfaces. A change to a locked item needs a decision from the captain.
`docs/reference/fh.md` has the full diagnostic tables. `docs/cad-recipe.md` has the CAD steps.

## The proof

Run `scripts/check-workflow.sh` from the repository root. It runs 3 cases and prints a pass or fail table:

- `validation/cases/bar-tension`: node sets on coordinate planes and an end displacement.
- `validation/cases/bar-pressure` (SI-mm) and `validation/cases/bar-pressure-m` (SI-m): 4 named regions and a pressure (`*DSLOAD`) on the region `LOAD`. Each case runs at 2 mesh sizes (8 mm and 4 mm).

The script needs `cargo`, gmsh and a Python with build123d. Set `PYTHON` and `FH_GMSH` to use other programs.
The script exits with 0 when each step passes and with 1 when a step fails. Set `KEEP=1` to keep the work folder.

| Step | Program | Result |
|---|---|---|
| `NAME-cad` | `scripts/cad/bar.py OUT.step mm\|m` (build123d) | `bar.step` in mm or in m |
| `NAME-step-units` | `fh info bar.step --case DIR` | The STEP unit matches the case |
| `NAME-mesh` | `fh mesh bar.step --case DIR --size H -o bar.msh` | `C3D10` mesh, a gmsh physical surface for each region, and `bar.msh.units` |
| `NAME-mesh-quality` | `fh info bar.msh --case DIR` | Positive Jacobian |
| `NAME-model` | `fh model bar.msh --case DIR -o bar.inp` | Solver input with the units line, the region surfaces and `*DSLOAD` |
| `NAME-check-inp` | `fh check bar.inp` | The parser accepts the input |
| `regions-coarse`, `regions-fine` | `grep` in `bar.inp` | 4 element-face surfaces at each mesh size |
| `refuse-*` | `fh info`, `fh mesh`, `fh model` | Exit 1 and the refusal code, for each input of the M2b gate (below) |
| `solve-validate-coarse`, `solve-validate-fine` | `fh validate --root ROOT` | The solver runs and the values match the references |

`NAME` is `tension`, `pressure-mm-coarse`, `pressure-m-coarse`, `pressure-mm-fine` or `pressure-m-fine`.
The refusal steps of the M2b gate:

| Step | Input | Refusal |
|---|---|---|
| `refuse-no-units` | A case with no `units` key | ``missing key `units` `` |
| `refuse-unit-mismatch` | A STEP file in mm with a case in `SI-m` | `unit.step.mismatch` |
| `refuse-step-no-unit` | A STEP file with no length unit record | `unit.step.missing` |
| `refuse-lost-region` | A region plane that touches no face of the part | `mesh.region.missing` |
| `refuse-renamed-region` | A region name in the case that the mesh does not hold | `mesh.region.missing` |
| `refuse-unnamed-volume` | A mesh volume with no physical group | `mesh.volume.unnamed` |
| `refuse-gmsh-failure` | A gmsh program that fails | `gmsh failed` |

Measured on macOS 27.0.1 arm64 (Apple M5) with gmsh 4.15.2, Python 3.12.13 and build123d 0.13.0.
All steps passed. One run took 9.6 s of elapsed time when the release build was up to date.
The values are equal at the 2 mesh sizes:

| Case | Quantity | Computed | Analytical reference | Tolerance |
|---|---|---:|---:|---:|
| `bar-tension` | `s_xx` (mean stress, MPa) | 2.1000e2 | 2.1e2 (E x strain) | 2.11e-4 |
| `bar-tension` | `rf_x` (sum of reactions, N) | -4.2000e4 | -4.2e4 (stress x area) | 4.2e-2 |
| `bar-pressure` | `s_xx` (mean stress, MPa) | 1.0000e2 | 1.0e2 (-pressure) | 1.0e-4 |
| `bar-pressure` | `rf_x` (sum of reactions, N) | -2.0000e4 | -2.0e4 (pressure x area) | 2.0e-2 |
| `bar-pressure` | `u_x_tip` (mean end displacement, mm) | 4.7619e-2 | 4.7619e-2 (stress / E x length) | 4.76e-8 |
| `bar-pressure-m` | `s_xx` (mean stress, Pa) | 1.0000e8 | 1.0e8 | 1.0e2 |
| `bar-pressure-m` | `rf_x` (sum of reactions, N) | -2.0000e4 | -2.0e4 | 2.0e-2 |
| `bar-pressure-m` | `u_x_tip` (mean end displacement, m) | 4.7619e-5 | 4.7619e-5 | 4.76e-11 |

The reaction sum balances the total pressure force, 100 MPa x 200 mm2 = 20000 N.
The cases in mm and in m give the same physical answer.

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
| `case.toml` | Schema 2 (`docs/reference/case-toml.md`). `units` is `"SI-mm"` or `"SI-m"` and has no default. The `[model]` table holds the step, the material, the node sets (`axis`, `at`), the named regions (`[[model.region]]`, by plane or by box), the boundaries, the `[[model.cload]]` loads and the `[[model.dsload]]` pressures. |
| `*.step` | STEP with one length unit record. `fh` reads the record as text. |
| `*.msh` | MSH 4.1 ASCII, complete quadratic tetrahedra (`C3D10`). Hex8 only for an extruded part. `fh mesh --case DIR` puts every volume in the physical group `SOLID` and makes a physical surface for each region of the case. |
| `*.msh.units` | One line, `SI-mm` or `SI-m`. `fh mesh` writes it. `fh model` reads it. |
| `*.inp` | The keyword subset in `docs/reference/inp.md`. `fh model` writes the line `** units: SI-mm` (or `SI-m`) before the first keyword, and each region as `*NSET` and `*SURFACE, TYPE=ELEMENT`. |
| Run folder | `results.pvd`, `step-N.vtu`, optional `step-N.txt` and `.vtk`, and `summary.toon` (`schema: fh.run`). `summary.toon`, `results.pvd` and the text results state the units of the input, or `unspecified` when the input has no units line. TOON numbers have 4 significant figures. The VTU files have full precision. |

## Exit codes

| Program | 0 | 1 | 2 | 130 |
|---|---|---|---|---|
| `fh mesh`, `fh info`, `fh model` | Success | A refusal or a failure of an external program | Usage error, or a case that cannot load (`fh mesh` returns 1 for this) | Not used |
| `fh check`, `fh validate` | All pass | A file or a case fails | Usage error, or no input found | Not used |
| `fh-solve` | Success | Solver failure | Usage error | Cancelled |

Refusal codes in the path: `unit.step.missing`, `unit.step.conflict`, `unit.step.mixed`, `unit.step.mismatch`,
`unit.mesh.missing`, `unit.mesh.unreadable`, `unit.mesh.conflict`, `unit.mesh.mismatch`, `mesh.empty`, `mesh.mixed`,
`mesh.jacobian.negative`, `mesh.volume.unnamed`, `mesh.region.missing`, `mesh.region.inner`, `model.missing`, `model.inp.invalid`.

## Open lock items

The lock is not complete. These items stay open in other tasks:

- The hex8 rule in the run command.
- The open parts of P1, E3, U1, E2 and the M2 stages (`docs/milestones.md`).
