# toon skill

## Purpose
- agent-facing summaries and queries (DESIGN.md section 11, row `toon`).

## Inputs and outputs
- Not defined in M0. The crate holds no logic yet.
- Licence role: open API. Line budget: 3k, tests included.

## Invariants the code must keep
- Stay inside the line budget of 3k (DESIGN.md section 11). A crate over budget needs a written reason before it grows.
- Licence role is "open API" (DESIGN.md section 11). Keep the crate inside that role.

## Forbidden actions
- Add a dependency without leader approval. Every dependency must use a permissive licence (AGENTS.md section 3, rule 4).
- Copy text, `.inp` files, figures, or numbers from Abaqus manuals (AGENTS.md section 4).

## Validation hooks it must pass
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-budget.sh` (AGENTS.md section 6).

## Examples
- None in M0.
