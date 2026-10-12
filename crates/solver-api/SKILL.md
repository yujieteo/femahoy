# solver-api skill

## Purpose
- The `Solver` trait, the subprocess protocol and the registry (DESIGN.md sections 9.1 and 11).
- Licence role: open API. Line budget: 3k, tests included (DESIGN.md section 11).

## Inputs and outputs
- `Solver` trait: a trusted Rust solver implements it and registers by name. The core solver is the reference implementation (DESIGN.md section 9.1).
- Subprocess protocol: an executable takes an `.inp` file path. It writes VTU and PVD results and a TOON summary (DESIGN.md section 9.1).
- The subprocess protocol is versioned. Protocol v1 is built in M2 (DESIGN.md sections 9.1 and 13).

## Invariants the code must keep
- Each subprocess run has its own working folder (DESIGN.md section 9.2).
- Each subprocess run has a time limit and a memory limit. The values are not yet decided (DESIGN.md section 9.2).
- Results from a solver that has not passed the validation suite are marked unvalidated (DESIGN.md section 9.2).
- The agents run under the harness's own permission system (DESIGN.md section 9.2).
- Line budget: 3k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Mark results from an unvalidated solver as validated (DESIGN.md section 9.2).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- M2 done-when: all linear fixtures pass, and the subprocess protocol v1 works (DESIGN.md section 13).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- Not yet decided.
