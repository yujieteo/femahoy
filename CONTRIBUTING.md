# Contributing to femahoy

This guide gives a short summary for human contributors.
The full rules for build agents are in `AGENTS.md`.

## Task rules

Make one change for each task.
Use one branch and one crate for each task.
Read the crate `SKILL.md` before you change code in that crate.
Do not add a dependency without approval.
Use only permissive licences: MIT, Apache-2.0, BSD, ISC or Zlib.
Never add GPL or AGPL code.
Keep each crate inside its line budget in `DESIGN.md` section 11.
Stop at the end of a milestone. Do not start the next milestone.

## Verification

For a bug, write a test that fails first.
Show the failing output. Fix the bug. Show the passing output.
For a new feature, write the test or fixture first. Then write the code.
For a solver change, run the convergence check for each changed element or contact path.
Include the convergence table in your report.
Every claim in a report needs its command and its output.
Do not write "should work" or "probably fixed".

## Merge gate

A change merges only when all these checks pass:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `fh validate`
- `fh validate --convergence <path>` for each changed solver path
- The tokei budget check, run by `scripts/check-budget.sh` in CI

After the checks pass, a reviewer reads the diff and the report.
A rejection from the reviewer blocks the merge.

## Licence

The code is licensed under AGPL-3.0.
A commercial licence needs no CLA text yet.
