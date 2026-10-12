# femahoy: TOON Output Schema

Status: agreed, version 1.0.0 of each schema. Implement in M1 (`fh`, `fh check`, `fh validate`) and later milestones (`fh run`, `fh results`, `fh solvers`).
Scope: everything `fh` prints on stdout, and the summary file that subprocess solvers write.
Rule source: our own rules, based on the AXI principles (Agent eXperience Interface, kunchenguid). Do not copy AXI text into the repo.

---

## 1. Rules

| # | Rule |
|---|---|
| R1 | **Encoder.** Use the `toon-format` crate (MIT), pinned to an exact version, behind a wrapper in the `toon` crate. No other crate calls `toon-format` directly. |
| R2 | **Header.** Every output starts with `schema`, `version`, `source`, `source_hash`, `units`, `status` (section 2.1). |
| R3 | **TOON is not data.** TOON is for reading and decisions only. Validation, submodelling, plots, and the viewer read VTU files at full precision. Numbers have 4 significant figures by default. `--precision N` changes this. |
| R4 | **Row limit.** A table shows at most 20 rows by default, ranked by the key stated for that table. `--rows N` changes the limit. No output prints per-node or per-element data for the whole model. |
| R5 | **Field limit.** A table has at most 4 fields. This rule has no exceptions, including with `--full`. |
| R6 | **`--full`.** Adds detail tables next to the main tables, joined by a shared key. It never adds fields to an existing table. |
| R7 | **Truncation.** The array length always equals the rows printed. A truncated table is followed by `<table>_more: <count>`. |
| R8 | **Empty tables.** A table with no rows prints as `name[0]:`. If the table has no data, it is followed by `<table>_note: <reason>`. If a row limit hid all rows, it is followed by `<table>_more: <count>` (R7) and has no note. Never omit a table that the schema lists. |
| R9 | **Names.** Use names from the `.inp` file: instance-qualified names (`Part-1-1.TOP`), and the user's node and element labels (`N3310`, `E1822`). Never print internal indices. |
| R10 | **Totals.** Every output has a `totals` block with pre-computed counts and statuses, so an agent can decide without reading the tables. |
| R11 | **Diagnostics.** Every output has a `diagnostics` table (section 2.3), even when it is empty. |
| R12 | **Next steps.** Every output ends with a `next` table of at most 3 rows: `{cmd,why}`. Each `cmd` can run as it is. |
| R13 | **Delimiter.** Comma by default. `--delim tab` is available. Measure the token counts of comma and tab on real outputs in M1, and change the default only if tab is clearly better. |
| R14 | **No prompts.** `fh` never asks for input. Unknown flags fail with exit code 2. |
| R15 | **Idempotent.** `fh init` is safe to run again. `fh run` with an unchanged `source_hash` reuses the results unless `--force` is given. |
| R16 | **Content first.** `fh` with no arguments prints `fh.status` (section 3.1). `fh --help` and `fh <cmd> --help` print short help text. |
| R17 | **Progress.** Progress goes to stderr, and only when stderr is a terminal. Stdout carries only the final TOON. |
| R18 | **Versioning.** Each schema has its own semantic version. Adding a field or a table is a minor change. Renaming or removing one is a major change. Snapshot tests on every schema protect the names. |

---

## 2. Common blocks

### 2.1 Header

```toon
schema: fh.check
version: 1.0.0
source: models/bracket.inp
source_hash: 9f3c1a
units: unspecified
status: ok
```

| Key | Meaning |
|---|---|
| `schema` | Schema name, `fh.<command>` |
| `version` | Schema version (R18) |
| `source` | The input file, results file, or project folder |
| `source_hash` | Short content hash of `source`. An agent compares it to see whether a summary is out of date. |
| `units` | Units as declared in the project, or `unspecified`. Abaqus-shaped input has no unit system. |
| `status` | `ok`, `warning`, `error`, `running`, `failed` (meaning depends on the schema) |

### 2.2 Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | The command ran, but the result is a failure: model errors, failed validation, no convergence |
| 2 | Usage error: unknown flag, missing file, bad argument |
| 3 | Internal error: a bug in `fh` |

### 2.3 Diagnostics

Main table (ranked: errors first, then warnings, then by line number):

```toon
diagnostics[1]{code,severity,line,message}:
  INP021,error,1204,unknown keyword *CONTACT CONTROLS
```

With `--full`, joined on `code` and `line`:

```toon
diagnostic_detail[1]{code,line,keyword,hint}:
  INP021,1204,*CONTACT CONTROLS,remove it or see fh docs keywords
```

The hint of the highest-ranked error also appears in `next`.

Code ranges. Each code is stable and listed in `docs/diagnostics.md`.

| Prefix | Area |
|---|---|
| `INP` | Parse and model structure |
| `MSH` | Mesh quality |
| `SOL` | Solver and convergence |
| `VAL` | Validation |
| `CLI` | Usage |
| `INT` | Internal errors |

### 2.4 Next steps

```toon
next[2]{cmd,why}:
  fh check models/bracket.inp --full,see the fix hint for INP021
  fh docs keywords,list supported keywords
```

---

## 3. Schemas

### 3.1 `fh.status` (`fh` with no arguments)

Also the output of the `SessionStart` hook that `fh init` installs.

```toon
schema: fh.status
version: 1.0.0
source: .
source_hash: 4be0d1
units: unspecified
status: warning
totals:
  models: 2
  runs: 3
  validation_pass: 12
  validation_fail: 1
  teammates_active: 1
models[2]{file,check,last_run,run_status}:
  models/bracket.inp,ok,r-0007,converged
  models/clip.inp,error,none,none
teammates[1]{name,role,state,task}:
  t-2,modeler,working,mesh clip with gmsh
next[2]{cmd,why}:
  fh check models/clip.inp,clip.inp has errors
  fh validate,one validation case fails
```

| Table | Rank |
|---|---|
| `models{file,check,last_run,run_status}` | Errors first, then by most recent run |
| `teammates{name,role,state,task}` | Blocked first, then working, then done |

### 3.2 `fh.check`

```toon
schema: fh.check
version: 1.0.0
source: models/bracket.inp
source_hash: 9f3c1a
units: unspecified
status: error
totals:
  nodes: 48210
  elements: 30112
  sets: 14
  materials: 1
  steps: 1
  errors: 1
  warnings: 2
element_types[2]{type,count}:
  C3D10,29800
  C3D8I,312
sets[3]{name,kind,size}:
  Part-1-1.FIXED,nset,412
  Part-1-1.LOAD,surface,96
  Part-1-1.ALL,elset,30112
sets_more: 11
materials[1]{name,behaviour,used_by}:
  STEEL,elastic-plastic,1
steps[1]{name,procedure,nlgeom,loads}:
  Step-1,static,no,2
quality[3]{metric,worst,limit,bad}:
  min_scaled_jacobian,0.21,0.1,0
  max_aspect_ratio,8.4,10,0
  inverted,0,0,0
diagnostics[1]{code,severity,line,message}:
  INP021,error,1204,unknown keyword *CONTACT CONTROLS
next[2]{cmd,why}:
  fh check models/bracket.inp --full,see the fix hint for INP021
  fh docs keywords,list supported keywords
```

| Table | Rank |
|---|---|
| `element_types{type,count}` | Count, largest first |
| `sets{name,kind,size}` | Sets used by steps first, then by name |
| `materials{name,behaviour,used_by}` | Name |
| `steps{name,procedure,nlgeom,loads}` | Input order |
| `quality{metric,worst,limit,bad}` | Fixed order |

Pre-solve checks, reported as diagnostics:

| Check | Severity |
|---|---|
| Element with no section | error |
| Section with no material | error |
| Load on an empty set or surface | error |
| Rigid-body motion not fully stopped in a connected mesh region (restrained-DOF count) | error |
| Inverted element (with its label) | error |
| Element below the quality limit (with its label) | warning |
| Set or surface never used | warning |

### 3.3 `fh.validate`

```toon
schema: fh.validate
version: 1.0.0
source: validation/cases
source_hash: c01d9e
units: unspecified
status: failed
totals:
  cases: 13
  pass: 12
  fail: 1
  skip: 0
cases[3]{id,class,status,margin}:
  hertz-sphere,N,fail,1.37
  lame-cylinder,L,pass,0.81
  cantilever-c3d8i,L,pass,0.42
cases_more: 10
next[1]{cmd,why}:
  fh validate hertz-sphere,see which quantity fails
```

- `margin` = the worst error in the case divided by its tolerance. A value below 1 is a pass. A value of 1 or more is a fail.
- Rank: failed cases first, then by margin, largest first.
- `status` in the header: `ok` if all pass, `failed` if any case fails.

### 3.4 `fh.validate.case` (`fh validate <id>`)

```toon
schema: fh.validate.case
version: 1.0.0
source: validation/cases/hertz-sphere
source_hash: 5e2f10
units: SI-mm
status: failed
totals:
  quantities: 3
  fail: 1
quantities[3]{name,value,reference,margin}:
  peak_pressure,1688,1592,1.37
  contact_radius,0.612,0.604,0.66
  total_force,1000,1000,0.0
next[1]{cmd,why}:
  fh validate hertz-sphere --full,see tolerances and mesh data
```

Extra tables by class:

| Class | Table |
|---|---|
| L | `refinement{mesh,size,value}` |
| R | `rates{field,observed,expected}` |
| `--full` (all) | `tolerance{name,tolerance,source}` |

### 3.5 `fh.run` (also `fh wait`)

```toon
schema: fh.run
version: 1.0.0
source: models/bracket.inp
source_hash: 9f3c1a
units: unspecified
status: ok
run_id: r-0007
solver: core
results: results/bracket.pvd
totals:
  steps: 1
  increments: 9
  cutbacks: 1
  iterations: 41
  wall_s: 38.2
steps[1]{name,status,reached,increments}:
  Step-1,converged,1.0,9
increments[2]{step,inc,iters,result}:
  Step-1,4,12,cutback
  Step-1,9,3,ok
increments_more: 7
contact[1]{pair,closed,max_pen,force}:
  CP-1,214,2.1e-4,1520
next[1]{cmd,why}:
  fh results,see extremes and reactions
```

Wait rule:
- `fh run` sends the job to the daemon and waits up to `--wait` seconds (default 60).
- If the job ends in time, the output is as above.
- If not, the output has `status: running`, the `run_id`, the `totals` so far, and `next: fh wait <run_id>`. `fh wait` uses the same rule.
- With no daemon, `fh run` blocks until the solve ends.

| Table | Rank |
|---|---|
| `steps{name,status,reached,increments}` | Input order |
| `increments{step,inc,iters,result}` | Failed and cutback increments first, then the last increment |
| `contact{pair,closed,max_pen,force}` | Only when contact exists. Largest penetration first. |

Extra tables when `status: failed`:

```toon
residuals[4]{iter,force,disp,contact_changes}:
  1,3.2e2,1.0,14
  2,8.1e1,4.2e-1,9
  3,7.9e1,4.0e-1,11
  4,8.3e1,4.4e-1,10
worst[2]{label,set,force}:
  N2207,Part-1-1.CONTACT_TOP,41.2
  N2210,Part-1-1.CONTACT_TOP,38.9
worst_more: 3
```

- `residuals` covers the last failed increment.
- `worst` lists the 5 nodes with the largest residual force.
- `next` gives the likely fix: a smaller first increment, a constraint check, or a contact surface check.

### 3.6 `fh.results`

No arguments:

```toon
schema: fh.results
version: 1.0.0
source: results/bracket.pvd
source_hash: 77ab02
units: unspecified
status: ok
at: integration_points
totals:
  steps: 1
  equilibrium_error: 3.1e-7
extremes[3]{field,value,at,step}:
  S.MISES,412.6,Part-1-1.E1822,Step-1
  U.MAG,0.842,Part-1-1.N3310,Step-1
  CPRESS,96.3,Part-1-1.N2207,Step-1
reactions[1]{set,rf1,rf2,rf3}:
  Part-1-1.FIXED,0,-1520,0
next[1]{cmd,why}:
  fh results --field S.MISES --set Part-1-1.ALL --top 10,see highest stresses
```

- `extremes` lists the fields present: `U.MAG` and `S.MISES` always, `PEEQ` if plastic, `CPRESS` if contact.
- `reactions` has one row per boundary-condition set.
- `equilibrium_error` = |applied load + total reaction| / |applied load|. A large value means the solve is wrong.
- `at` in the header: `integration_points` (default) or `nodes` (with `--at nodes`, extrapolated and averaged).

Query (`fh results --field S.MISES --set Part-1-1.ALL --step Step-1 --top 10`):

```toon
schema: fh.results.query
version: 1.0.0
source: results/bracket.pvd
source_hash: 77ab02
units: unspecified
status: ok
at: integration_points
field: S.MISES
set: Part-1-1.ALL
step: Step-1
totals:
  matched: 30112
  max: 412.6
  min: 0.31
rows[2]{label,value}:
  E1822,412.6
  E1823,398.1
rows_more: 8
next[1]{cmd,why}:
  fh results --field S.MISES --set Part-1-1.ALL --at nodes --top 10,compare averaged nodal values
```

Flags: `--field`, `--set`, `--step`, `--top N`, `--bottom N`, `--at integration_points|nodes`. No query language.

### 3.7 `fh.solvers`

```toon
schema: fh.solvers
version: 1.0.0
source: .
source_hash: 4be0d1
units: unspecified
status: ok
totals:
  solvers: 2
  validated: 1
solvers[2]{name,kind,validated,version}:
  core,trait,yes,0.1.0
  my-explicit,subprocess,no,0.0.3
next[1]{cmd,why}:
  fh validate --solver my-explicit,validate the user solver
```

### 3.8 Subprocess solver summary

A subprocess solver writes VTU and PVD results and an `fh.run` summary file (section 3.5) next to them. `solver` in the header is its registered name. `fh` checks the summary against this schema. A summary that fails the check makes the run `status: failed` with a `SOL` diagnostic.

---

## 4. Field names

Abaqus output names with a component suffix.

| Field | Components |
|---|---|
| `U` | `U.U1`, `U.U2`, `U.U3`, `U.MAG` |
| `S` | `S.S11` ... `S.S23`, `S.MISES`, `S.MAX_PRINCIPAL`, `S.MIN_PRINCIPAL`, `S.PRESS` |
| `E` | `E.E11` ... `E.E23` |
| `PEEQ` | (scalar) |
| `RF` | `RF.RF1`, `RF.RF2`, `RF.RF3`, `RF.MAG` |
| `CPRESS` | (scalar) |
| `COPEN` | (scalar) |

---

## 5. Tests

- One snapshot test per schema, with the outputs in section 3 as the starting snapshots.
- A test checks that every table has 4 fields or fewer (R5).
- A test checks that every array length equals its rows (R7), using strict `toon-format` decoding.
- A test checks that every schema has `totals`, `diagnostics`, and `next` (R10 to R12).

## 6. Open

| Item | When |
|---|---|
| Comma or tab default (R13), measured on real outputs | M1 |
| Codex startup hook support (for the `SessionStart` equivalent) | M4 |
| Threshold filter (`--above`, `--below`) for queries | When agents need it |
