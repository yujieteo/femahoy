# io skill

## Purpose
- gmsh `.msh` import. VTU and PVD write and read (DESIGN.md sections 6.2, 8 and 11).
- Licence role: open API. Line budget: 5k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: `.msh` files from gmsh, and `.inp` meshes from other tools (DESIGN.md section 6.2).
- Output: one VTU file per step, plus one PVD collection file (DESIGN.md section 8).
- ParaView opens the PVD file directly (DESIGN.md section 8).

## Invariants the code must keep
- Binary data is appended raw. Sets are stored as integer arrays (DESIGN.md section 8).
- Read and write run in Rust. No C library is used (DESIGN.md section 8).
- The wasm viewer and the submodel interpolation read the same files (DESIGN.md section 8).
- Line budget: 5k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Link gmsh. gmsh is GPL. femahoy calls it only as an external program (DESIGN.md sections 6.2 and 11.1).
- Bundle gmsh. The user installs gmsh (DESIGN.md section 6.2).
- Add a mesher to this crate. femahoy has no mesher of its own (DESIGN.md section 6.2).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- M1 done-when: `fh check` works on all fixtures, including the gmsh import (DESIGN.md section 13).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- Not yet decided.
