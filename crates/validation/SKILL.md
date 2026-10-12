# validation skill

## Purpose
- The validation harness, the tolerance classes and the manufactured solutions (DESIGN.md sections 10 and 11).
- Licence role: validated core. Line budget: 6k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: fixtures in `validation/cases/<id>/`. Each holds the input files, the reference values, the tolerance class and the justification (DESIGN.md section 10.4).
- Output: `fh validate` reports pass or fail for each case (DESIGN.md section 10.5).
- Fixture schema: see `crates/validation/SCHEMA.md`.

## Invariants the code must keep
- Fixtures are data files, not prose (DESIGN.md section 10.4).
- Tolerance class L: Richardson extrapolation over at least three mesh refinements (DESIGN.md section 10.3).
- Tolerance class N: a fixed tolerance per case, written and justified in the fixture file (DESIGN.md section 10.3).
- Tolerance class R: the fixture states the expected convergence rate and the allowed band (DESIGN.md section 10.3).
- A failure blocks the merge. There is no override (DESIGN.md section 10.5).
- Line budget: 6k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Run Abaqus. No Abaqus licence is available (DESIGN.md section 10.1).
- Add NAFEMS target values before the leader confirms the legal check (AGENTS.md section 4).
- Weaken, skip or delete a validation case. Change a tolerance only with a written reason and leader approval (AGENTS.md section 4).

## Validation hooks it must pass
- `fh validate` runs the suite. M1 done-when: it reports "no solver" (DESIGN.md sections 10.5 and 13).
- For each changed solver path: `fh validate --convergence <path>` (DESIGN.md section 10.5, AGENTS.md section 6).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- Cases from DESIGN.md section 10.2 include the patch test, the cantilever, the Lamé thick cylinder and the Hertz sphere on a flat plane.
