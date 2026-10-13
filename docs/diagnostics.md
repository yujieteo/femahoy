# Diagnostic codes

Each error of the `inp`, `io` and `solver-api` crates has a stable code.
A code never changes its meaning. A new error gets a new code.

An error shows as `[file: ]position: message (CODE)`.
The `file` field names the file when the caller knows it.

## Collect many errors

- `fh_inp::parse_all(text)` returns all errors of all keyword blocks, in line order.
- `fh_inp::parse_file(name, text)` does the same and puts `name` in each error.
- `fh_inp::parse(text)` returns only the first error.
- A block reports its first error only. An error in one block can cause errors in later blocks, for example an undefined node.
- `fh_io::vtu::check_grid(grid)` returns all problems of a grid.
- The runner reports all missing VTU files of a PVD file in one `API008` error.
- The `.msh` reader and the other runner checks stop at the first error. `fh_io::MshError::with_file` names the file.

## INP: `.inp` reader (`fh-inp`)

| Code | Meaning |
|---|---|
| INP001 | Syntax error: a keyword line has no name, or a data line comes before the first keyword. |
| INP002 | Unsupported keyword. |
| INP003 | Unsupported parameter. |
| INP004 | A parameter is missing or has no value. |
| INP005 | Unsupported value, for example an element type, a load type or `TYPE`. |
| INP006 | A value is not valid: not a number, out of range, or not in the allowed set. |
| INP007 | A data line has the wrong shape or number of fields. |
| INP008 | A name or an id is defined twice. |
| INP009 | A name or an id is not defined. |
| INP010 | A keyword is in the wrong place, for example outside `*STEP`. |
| INP011 | A block is not complete, for example a step without `*END STEP`. |

## MSH: gmsh import (`fh-io`)

| Code | Meaning |
|---|---|
| MSH001 | Syntax error in the `.msh` file. |
| MSH002 | Unsupported feature: version, binary format or element type. |
| MSH003 | A node or an element is defined twice. |
| MSH004 | An element uses a node that is not defined. |

## VTU: VTU, PVD and VTK files (`fh-io`)

| Code | Meaning |
|---|---|
| VTU001 | The grid data is not consistent, so the grid cannot be written. |
| VTU002 | A VTU or PVD file is malformed. |
| VTU003 | Unsupported feature, for example an array type or an array over 4 GiB. |
| VTU004 | A write to the output failed. |

## API: subprocess runner (`fh-solver-api`)

| Code | Meaning |
|---|---|
| API001 | The run was cancelled. |
| API002 | The run folder is not usable, or the runner cannot create a file in it. |
| API003 | The runner cannot start the solver or read the input file. |
| API004 | The solver does not follow the protocol. |
| API005 | The memory limit is passed, or the runner cannot enforce it. |
| API006 | The time limit is passed. |
| API007 | The solver failed. |
| API008 | A result file (`summary.toon`, `results.pvd`) is missing or not valid. |
| API009 | A path leads out of the run folder. |
| API010 | The solver cannot write or read its own output, restart or model data. |
| API011 | A solver name is registered twice. |

`StepUnitError` in `fh-io` already has its own codes (`unit.step.*`). `SolverError` in `core-solver` has no code yet.
