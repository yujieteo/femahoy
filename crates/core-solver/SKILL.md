# core-solver skill

## Purpose
- Elements, DOF elimination, Newton with cutback, contact, plasticity, NLGEOM, birth and death, submodelling (DESIGN.md section 11).
- Static analysis, implicit and general, in the Abaqus style (DESIGN.md section 7.1).
- Licence role: validated core. Line budget: 30k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: the Abaqus-shaped model from `inp` (DESIGN.md section 6.1).
- Output: results per step, written as VTU and PVD files by `io` (DESIGN.md section 8).
- Elements: `C3D8I` (default hex8), `C3D8` (B-bar, for plasticity), `C3D10` (default tet10), `C3D10M` (contact surfaces) (DESIGN.md section 7.2).
- For models that agents create, use `C3D8I` and `C3D10` unless the task needs others (DESIGN.md section 7.2).

## Invariants the code must keep
- DOF elimination keeps the matrix symmetric positive definite (DESIGN.md section 7.1).
- A removed element leaves the stiffness. Its stresses are set to zero. The mesh stays fixed (DESIGN.md section 7.1).
- Submodelling interpolates global displacements at the driven nodes with the global mesh shape functions (DESIGN.md section 7.1).
- Contact cases compare integrated values only: total contact force, contact area or radius, and peak pressure (DESIGN.md section 10.3).
- Tolerance classes L, N and R apply as DESIGN.md section 10.3 states.
- Line budget: 30k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Use a Haiku model for numerical work in this crate. Stop and report to the leader (AGENTS.md section 2).
- Compare node-by-node contact status in a test (DESIGN.md section 10.3).
- Weaken, skip or delete a validation case. Change a tolerance only with a written reason and leader approval (AGENTS.md section 4).
- Copy text, `.inp` files, figures or numbers from Abaqus manuals (DESIGN.md section 10.1).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- For each changed element or contact path: `fh validate --convergence <path>` (DESIGN.md section 10.5, AGENTS.md section 6, from M1).
- The full validation suite passes before a solver change merges (DESIGN.md section 10.5).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- Not yet decided. The first solver milestone is M2 (DESIGN.md section 13).
