# Compatibility

This file lists how femahoy differs from Abaqus input conventions.

## Rule

- Keyword names and meanings match Abaqus for the supported subset.
- Every difference is listed in this file.
- A test covers each difference.
- Tests use femahoy `.inp` files. No Abaqus manual file is in the repo.

## Differences

| Keyword | Difference | Test |
|---|---|---|
| `*SURFACE` | Only `TYPE=NODE`. The data lines are node numbers or node set names. | `crates/inp/tests/m2_keywords.rs` `node_surface_takes_nodes_and_sets` |
| `*DISTRIBUTING` | All slave nodes have the same weight. The data line gives only the first and last dof. | `crates/inp/tests/m2_keywords.rs` `coupling_without_data_uses_all_dofs` |
| `*EQUATION` | The sum of the terms is always zero. Terms use node numbers, not node set names. | `crates/inp/tests/m2_keywords.rs` `equation_reads_terms_over_lines` |
| `*MPC` | Only the types `TIE`, `PIN`, `LINK` and `BEAM`, each with 2 nodes. | `crates/inp/tests/m2_keywords.rs` `mpc_types` |
| `*CLOAD` | Data lines for the same node and dof in one step add up. A later step replaces the total of the earlier step. | `crates/core-solver/tests/review.rs` `repeated_cload_on_one_dof_adds_inside_a_step` |
| `*DLOAD` | Only pressure on an element face (`P1` to `P6`). | `crates/inp/tests/m2_keywords.rs` `dload_pressure_on_face` |

## Keywords for M1

M1 supports these keywords first:

- `*NODE`
- `*ELEMENT`
- `*NSET`
- `*ELSET`
- `*MATERIAL`
- `*ELASTIC`
- `*STEP`
- `*STATIC`
- `*BOUNDARY`
- `*CLOAD`

## Keywords added for M2

- `*SOLID SECTION`
- `*SURFACE`
- `*COUPLING` with `*KINEMATIC` or `*DISTRIBUTING`
- `*EQUATION`
- `*MPC`
- `*DLOAD`
