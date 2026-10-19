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
| `*SURFACE` | Only `TYPE=ELEMENT`. Each data line is `element or element set, S<face>`, with face 1 to 4. `*DSLOAD` reads `surface, P, magnitude` in a step. The solver applies the pressure on each face of the element surface. `TYPE=NODE` fails with error `INP005`. | `crates/inp/tests/m2_keywords.rs` `element_surface_takes_elements_and_sets`, `crates/inp/src/parse.rs` `a_node_surface_is_rejected` |
| `*COUPLING`, `*KINEMATIC`, `*DISTRIBUTING`, `*EQUATION`, `*MPC` | Not supported. Each fails with error `INP002`. | `crates/inp/src/parse.rs` `contact_and_constraint_keywords_are_rejected` |
| `*STEP` | Only the parameter `NAME`. `NLGEOM` fails with error `INP003`. | `crates/inp/src/parse.rs` `step_rules` |
| `*STATIC` | The parser does not read the data lines. Each step is 1 linear increment. A second `*STATIC` in a step fails with error `INP008`. | `crates/inp/src/parse.rs` `step_rules` |
| `*ELEMENT` | Only `TYPE=C3D10`. Another type fails with error `INP005`. | `crates/solver-ccx/tests/adapter.rs` `a_keyword_outside_the_subset_is_a_clear_error` |
| `*PART`, `*ASSEMBLY`, `*INSTANCE` | Not supported. The model must be flat, with global node and element numbers. Each of these keywords and its `*END` keyword fails with error `INP002`. | `crates/inp/tests/parts.rs` `part_assembly_and_instance_fail_with_a_clear_error` |
| `*CLOAD` | Data lines for the same node and dof in one step add up. A later step replaces the total of the earlier step. | `crates/solver-ccx/tests/adapter.rs` `real_ccx_adds_cload_lines_in_a_step_and_a_later_step_replaces_the_total` |
| `*DLOAD` | Only pressure on an element face (`P1` to `P4`). | `crates/inp/tests/m2_keywords.rs` `dload_pressure_on_face` |
| `*EXPANSION` | Only `TYPE=ISO` with 1 value. No `ZERO` parameter and no temperature table. The thermal strain is alpha (T - T0), with T0 the initial temperature of the node. | `crates/inp/src/parse.rs` `thermal_keyword_rules` |
| `*INITIAL CONDITIONS` | Only `TYPE=TEMPERATURE`, with 1 temperature for each node or node set. | `crates/inp/src/parse.rs` `thermal_keyword_rules` |
| `*TEMPERATURE` | No parameters. Each line gives 1 temperature for a node or a node set. The adapter gives it to CalculiX. | `crates/inp/src/parse.rs` `thermal_keyword_rules`, `crates/solver-ccx/tests/adapter.rs` `the_thermal_keywords_go_to_ccx` |

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
- `*DLOAD`
