# Diagnostic codes

Each error of the `inp`, `io` and `solver-ccx` crates has a stable code.
A code never changes its meaning. A new error gets a new code.

An error shows as `[file: ]position: message (CODE)`.
The `file` field names the file when the caller knows it.

## Collect many errors

- `fh_inp::parse_all(text)` returns all errors of all keyword blocks, in line order.
- `fh_inp::parse_file(name, text)` does the same and puts `name` in each error.
- `fh_inp::parse(text)` returns only the first error.
- A block reports its first error only. An error in one block can cause errors in later blocks, for example an undefined node.
- `fh_io::vtu` and `fh_io::vtk` refuse a grid at its first problem.
- The `.msh` reader and the `ccx` run stop at the first error. `fh_io::MshError::with_file` names the file.

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

## API: `ccx` run (`fh-solver-ccx`)

| Code | Meaning |
|---|---|
| API001 | The run was cancelled. |
| API002 | The run folder is not usable, or the run cannot create a file in it. |
| API003 | The run cannot start `ccx` or read the input file. |
| API004 | Retired on 2026-10-19 with protocol v2 (D-31). It was a solver that did not follow the protocol. |
| API005 | The memory limit is passed, or the run cannot enforce it. |
| API006 | The time limit is passed. |
| API007 | `ccx` failed, or its result files are not valid. |
| API008 | Retired on 2026-10-19 with protocol v2 (D-31). It was a result file that was missing or not valid. |
| API009 | Retired on 2026-10-19 with protocol v2 (D-31). It was a path that led out of the run folder. |
| API010 | Retired on 2026-10-19 with the native solver. It was an output, restart or model data fault of an in-process solver. |
| API011 | Retired on 2026-10-19 with the solver registry. It was a solver name that was registered twice. |

`StepUnitError` in `fh-io` already has its own codes (`unit.step.*`).
