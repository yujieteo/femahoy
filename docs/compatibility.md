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
| `*SURFACE` | Only `TYPE=NODE` and `TYPE=ELEMENT`. Each data line of a node surface is 1 node number or 1 node set name, without a weight. An element surface takes `element or element set, S<face>` lines, and the model stores them. A coupling can name either kind. `*DSLOAD` reads `surface, P, magnitude` in a step, and `solve` returns an error for it because the load is not applied yet. | `crates/inp/tests/m2_keywords.rs` `node_surface_takes_nodes_and_sets`, `a_surface_line_with_more_than_1_field_is_an_error`, `element_surface_takes_elements_and_sets` |
| `*KINEMATIC` | A coupling is an error when no slave node has a dof in its range. For example, the range 4 to 6 on slave nodes without rotation dofs. | `crates/core-solver/tests/review.rs` `a_kinematic_coupling_turns_the_slave_rotations_with_the_reference` |
| `*PART`, `*ASSEMBLY`, `*INSTANCE` | The reader flattens instances into global node and element numbers. `Model::node_instance` and `element_instance` give the instance and part label. One `*ASSEMBLY`. A part holds `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET` and `*SOLID SECTION`. An instance takes a translation only. Part sets are named `INSTANCE.SET`. | `crates/inp/tests/parts.rs` `maps_name_the_instance_and_part_label` |
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

## C3D8 mesh rule for bending

C3D8 uses a B-bar formulation.
With 1 element through the section, it is too soft in bending.
Refinement along the beam alone does not remove this error.
Use multiple elements through the section and check convergence.
The C3D8 cantilever fixture uses 4 elements through each section axis.
It checks axial refinement with the original beam reference and tolerances.
