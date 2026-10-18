# agents skill

## Purpose

The crate is a placeholder. `src/lib.rs` contains no implementation. M4 (`docs/milestones.md`) builds the leader start, the JSON stream parser and the skill installer here.

## Inputs and outputs

No harness, role config, JSON stream parser or skill installer exists in this crate.

## Invariants the code must keep

`fh init`, `fh team` and `fh skills` are not implemented in `cli/src/main.rs`.
- The validator runs as a separate process from the leader (DESIGN.md section 5.2).
- API keys go to `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` in the process environment only (DESIGN.md section 5.1).

## Forbidden actions

- Do not describe these features as available.
- Add an agent loop to femahoy. Every agent is a CLI harness process (DESIGN.md section 5.1).
- Send an API key to the browser (DESIGN.md section 4.1).

## Validation hooks it must pass

Run `cargo check -p fh-agents` and `cargo test -p fh-agents`.

## Examples

Run `fh --help` to check the available commands.
