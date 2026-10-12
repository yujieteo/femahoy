# toon skill

## Purpose
- Agent-facing summaries and queries (DESIGN.md sections 8 and 11).
- Licence role: open API. Line budget: 3k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: a summary or query that an agent asks for (DESIGN.md section 8).
- Output: TOON text. `fh check`, `fh results` and the solver subprocess summary use it (DESIGN.md sections 6.1, 8 and 9.1).
- TOON summary schema v1: decided in M1 (DESIGN.md section 13).

## Invariants the code must keep
- TOON holds summaries and query answers only (DESIGN.md section 8).
- Full field data never goes into TOON (DESIGN.md section 8).
- Line budget: 3k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Write full field arrays into TOON (DESIGN.md section 8).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- M1 done-when: `fh check` works on all fixtures, with the TOON summary schema v1 (DESIGN.md section 13).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- Query: "maximum Mises stress in set X, step 2" (DESIGN.md section 8).
