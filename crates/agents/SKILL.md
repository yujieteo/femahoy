# agents skill

## Purpose
- Start the harness processes: Claude Code or Codex. femahoy has no agent loop of its own (DESIGN.md section 5.1).
- Parse the JSON event stream from the leader (DESIGN.md section 5.1).
- Install skills and read role config (DESIGN.md sections 5.2 and 5.3).
- Licence role: app. Line budget: 4k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: `femahoy.toml`, which sets the harness and model for each role (DESIGN.md section 5.2).
- Input: skill sources at `.femahoy/skills/<role>/SKILL.md` (DESIGN.md section 5.3).
- Output: harness processes. The leader runs headless with a JSON event stream. Teammates run in panes through `PaneBackend` (DESIGN.md section 5.1).
- Output: `.claude/skills/` entries and an `AGENTS.md` that points Codex to the same files (DESIGN.md section 5.3).
- Exact leader flags: not yet decided. Check them against the current harness docs in M4 (DESIGN.md section 5.1).

## Invariants the code must keep
- Two connection paths only in v1: the user's CLI login, and the user's API key in the process environment (DESIGN.md section 5.1).
- API keys go to `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` in the process environment only (DESIGN.md section 5.1).
- The validator runs as a separate process from the leader (DESIGN.md section 5.2).
- When both vendors are available, the validator uses the other vendor (DESIGN.md section 5.2).
- Each skill file has these sections: purpose, inputs and outputs, invariants the code must keep, forbidden actions, validation hooks it must pass, examples (DESIGN.md section 5.3).
- Skills follow the `fh` binary version. `fh skills update` refreshes them (DESIGN.md section 5.3).

## Forbidden actions
- Add an agent loop to femahoy. Every agent is a CLI harness process (DESIGN.md section 5.1).
- Send an API key to the browser. The browser never holds an API key (DESIGN.md section 4.1).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- The M4 done-when test: the leader builds and solves a cantilever model from one chat request (DESIGN.md section 13).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- `fh team spawn` starts a teammate. Each start is a visible command (DESIGN.md section 5.1).
- `fh init` installs the harness wiring for skills (DESIGN.md section 5.3).
- `fh skills update` refreshes the skills (DESIGN.md section 5.3).
