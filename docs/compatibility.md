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
| `*SURFACE` | Only `TYPE=NODE` and `TYPE=ELEMENT`. Each data line of a node surface is 1 node number or 1 node set name, without a weight. An element surface takes `element or element set, S<face>` lines, and the model stores them. A coupling can name either kind. `*DSLOAD` reads `surface, P, magnitude` in a step. The solver applies the pressure on each face of the element surface. | `crates/inp/tests/m2_keywords.rs` `node_surface_takes_nodes_and_sets`, `a_surface_line_with_more_than_1_field_is_an_error`, `element_surface_takes_elements_and_sets` |
| `*KINEMATIC` | A coupling is an error when no slave node has a dof in its range. For example, the range 4 to 6 on slave nodes without rotation dofs. | `crates/core-solver/tests/review.rs` `a_kinematic_coupling_turns_the_slave_rotations_with_the_reference` |
| `*PART`, `*ASSEMBLY`, `*INSTANCE` | Not supported. The model must be flat, with global node and element numbers. Each of these keywords and its `*END` keyword fails with error `INP002`. | `crates/inp/tests/parts.rs` `part_assembly_and_instance_fail_with_a_clear_error` |
| `*DISTRIBUTING` | All slave nodes have the same weight. The data line gives only the first and last dof. | `crates/inp/tests/m2_keywords.rs` `coupling_without_data_uses_all_dofs` |
| `*EQUATION` | The sum of the terms is always zero. Terms use node numbers, not node set names. | `crates/inp/tests/m2_keywords.rs` `equation_reads_terms_over_lines` |
| `*MPC` | Only the types `TIE`, `PIN`, `LINK` and `BEAM`, each with 2 nodes. | `crates/inp/tests/m2_keywords.rs` `mpc_types` |
| `*CLOAD` | Data lines for the same node and dof in one step add up. A later step replaces the total of the earlier step. | `crates/core-solver/tests/review.rs` `repeated_cload_on_one_dof_adds_inside_a_step` |
| `*DLOAD` | Only pressure on an element face (`P1` to `P6`). | `crates/inp/tests/m2_keywords.rs` `dload_pressure_on_face` |
| `*EXPANSION` | Only `TYPE=ISO` with 1 value. No `ZERO` parameter and no temperature table. The thermal strain is alpha (T - T0), with T0 the initial temperature of the node. | `crates/inp/src/parse.rs` `thermal_keyword_rules` |
| `*INITIAL CONDITIONS` | Only `TYPE=TEMPERATURE`, with 1 temperature for each node or node set. | `crates/inp/src/parse.rs` `thermal_keyword_rules` |
| `*TEMPERATURE` | No parameters. Each line gives 1 temperature for a node or a node set. `fh-solve` refuses thermal input. `fh-solve-ccx` gives it to CalculiX. | `crates/inp/src/parse.rs` `thermal_keyword_rules`, `crates/core-solver/tests/review.rs` `thermal_input_is_refused_with_the_keyword_and_the_ccx_solver` |

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

`C3D8` uses the B-bar method (Hughes 1980). Each integration point uses the element mean of the volume strain.
With 1 element through the depth of a section in bending, the result depends on the element length and does not converge to the beam value.

### Rule

1. Do not use 1 `C3D8` element through the depth of a section in bending.
2. Use at least 4 `C3D8` elements through the depth and 4 across the width.
3. Make the element length along the bending span 0.5 to 1 times the element depth.
4. Check convergence with refinement in all 3 directions, not along the span only.
5. For a thin extruded part in bending, use `C3D8I`. With box-shaped elements, 1 `C3D8I` element through the depth gives 0.96 to 0.99 times the beam value.

### Data

The table gives the tip deflection of a cantilever divided by the Timoshenko beam value (shear factor 5/6).
The cantilever has the length 8, a 1 x 1 section, a fixed end and a tip force. Poisson's ratio is 0.3.
The 3D fixed end makes the converged value about 0.99 times the beam value.

| Mesh (along, width, depth) | Element length / element depth | `C3D8` | `C3D8I` |
|---|---:|---:|---:|
| 4 x 1 x 1 | 2 | 0.50 | 0.96 |
| 8 x 1 x 1 | 1 | 1.19 | 0.98 |
| 16 x 1 x 1 | 0.5 | 1.83 | 0.99 |
| 64 x 1 x 1 | 0.125 | 2.20 | 0.99 |
| 16 x 2 x 2 | 1 | 0.97 | 0.98 |
| 32 x 2 x 2 | 0.5 | 1.05 | 0.99 |
| 32 x 1 x 4 | 1 | 0.97 | 0.99 |
| 16 x 4 x 4 | 2 | 0.92 | 0.98 |
| 32 x 4 x 4 | 1 | 0.98 | 0.99 |
| 64 x 4 x 4 | 0.5 | 1.00 | 0.99 |

The `cantilever` validation case has the length 10 mm, a 1 x 1 mm section, Poisson's ratio 0.3 and 1 element through the section.
With the element type changed from `C3D8I` to `C3D8`, 4 elements along the beam (`fixtures/inp/cantilever.inp`) give a tip U3 of -6.682931e-3 mm, which is 0.348 times the reference -1.919619048e-2 mm.
With 32 elements (`validation/cases/cantilever/mesh-32.inp`), the tip U3 is -3.968578e-2 mm, which is 2.067 times the reference.

### Cause

The element review (task T9) found no defect. The `C3D8` stiffness is equal to the Hughes form of B-bar to a relative difference of 1e-15.
With 1 element through the depth, 2 effects control the bending stiffness:

- **Long elements are too stiff.** The 2 x 2 x 2 points see an in-plane shear strain that is not in the beam (shear locking). It adds `mu (a / h)^2` to the bending modulus, with element length `a`, depth `h` and shear modulus `mu`.
- **Short elements are too soft.** In bending, the volume strain changes linearly through the depth. Its element mean is 0, so B-bar removes the volumetric stiffness. The bending modulus becomes `4 mu (1 + r) / (4 + 3 r)` with `r = (width / depth)^2`. For a square section and Poisson's ratio 0.3, this is 0.44 times Young's modulus. Thus the bending deflection goes to 2.26 times the beam value, or about 2.23 times with the 3D fixed end.

A fully integrated hex8 has the opposite limit. In a probe with 64 elements along the span and 1 through the section, it stayed too stiff, at 0.86 times the beam value.
`C3D8I` has incompatible modes that represent the pure bending field of a box-shaped element exactly.

Tests:

- `crates/core-solver/tests/elements.rs`: `hex8_stiffness_has_6_zero_modes_and_no_negative_mode`, `hex8_bending_energy_matches_the_formulation` and `c3d8_pressure_is_the_bulk_modulus_times_the_volume_change`.
- `crates/core-solver/tests/beam.rs`: `one_c3d8_through_the_section_follows_the_b_bar_model`.

The `C3D8` cantilever fixture (`validation/cases/cantilever-c3d8`) uses 4 elements through each section axis.
It checks axial refinement with the original beam reference and tolerances.
