# core-solver skill

## Purpose
- elements, DOF elimination, Newton with cutback, contact, plasticity, NLGEOM, birth and death, submodelling (DESIGN.md section 11, row `core-solver`).

## Inputs and outputs
- Not defined in M0. The crate holds no logic yet.
- Licence role: validated core. Line budget: 30k, tests included.

## Invariants the code must keep
- Stay inside the line budget of 30k (DESIGN.md section 11). A crate over budget needs a written reason before it grows.
- Licence role is "validated core" (DESIGN.md section 11). Keep the crate inside that role.

## Forbidden actions
- Add a dependency without leader approval. Every dependency must use a permissive licence (AGENTS.md section 3, rule 4).
- Copy text, `.inp` files, figures, or numbers from Abaqus manuals (AGENTS.md section 4).
- Numerical code is for Opus 5.5 or Sonnet 5.5 only. A Haiku task that enters this area must stop and report (AGENTS.md section 2).
- Do not weaken, skip, or delete a validation case, and do not change a tolerance without a written reason and leader approval (AGENTS.md section 4).

## Validation hooks it must pass
- For each changed solver path, `fh validate --convergence <path>` (AGENTS.md section 6, from M1).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-budget.sh` (AGENTS.md section 6).

## Examples
- None in M0.
