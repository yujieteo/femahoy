# validation skill

## Purpose
- harness, tolerance classes, manufactured solutions (DESIGN.md section 11, row `validation`).

## Inputs and outputs
- Not defined in M0. The crate holds no logic yet.
- Licence role: validated core. Line budget: 6k, tests included.

## Invariants the code must keep
- Stay inside the line budget of 6k (DESIGN.md section 11). A crate over budget needs a written reason before it grows.
- Licence role is "validated core" (DESIGN.md section 11). Keep the crate inside that role.

## Forbidden actions
- Add a dependency without leader approval. Every dependency must use a permissive licence (AGENTS.md section 3, rule 4).
- Copy text, `.inp` files, figures, or numbers from Abaqus manuals (AGENTS.md section 4).
- Do not weaken, skip, or delete a validation case, and do not change a tolerance without a written reason and leader approval (AGENTS.md section 4).

## Validation hooks it must pass
- For each changed solver path, `fh validate --convergence <path>` (AGENTS.md section 6, from M1).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-budget.sh` (AGENTS.md section 6).

## Examples
- None in M0.
