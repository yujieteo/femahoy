# io skill

## Purpose

Read gmsh meshes, scan STEP units, check mesh quality, and read or write result files.

## Inputs and outputs

- `read_msh` accepts MSH 4.1 ASCII and returns `fh_inp::Model`.
- Imported volume types are gmsh 5 (`C3D8I`) and 11 (`C3D10`).
- `step_length_unit` scans text. `mesh_stats` reports bounds, volume, element counts and corner Jacobian quality.
- `vtu` reads and writes VTU/PVD. `vtk` writes legacy VTK.

## Invariants the code must keep

- gmsh is the mesher now, as an external program. A mesher of our own is a main advantage for later, with no milestone yet (D-15).
- VTU uses appended raw data with little-endian `UInt32` byte counts.
- Float64 arrays hold fields. Int32 arrays hold sets and IDs.
- STEP unit errors distinguish missing records from conflicting records.
- Read and write run in Rust. No C library is used (DESIGN.md section 8).

## Forbidden actions

- Do not imply that the STEP scanner imports geometry or that the mesh reader creates material and load definitions.
- Bundle gmsh. The user installs gmsh (DESIGN.md section 6.2).

## Validation hooks it must pass

Run `cargo check -p fh-io` and `cargo test -p fh-io`. Use `fh info` to check units and mesh quality.

## Examples

`fh info part.msh` reports mesh quality. `fh info part.step --case validation/cases/cantilever` checks declared units.
