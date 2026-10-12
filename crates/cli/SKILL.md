# cli skill

## Purpose
- The `fh` command line. It works alone over SSH and tmux, with no GUI (DESIGN.md section 3).
- It talks to a running daemon. It runs the solver directly when no daemon runs (DESIGN.md section 4.2).
- Licence role: app. Line budget: 3k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: command line arguments.
- Implemented commands: `check`, `validate` (`fh validate [--root DIR]`; fixture schema in `crates/validation/SCHEMA.md`).
- Planned commands: `results`, `plot`, `serve`, `init`, `team`, `skills` (DESIGN.md sections 5 to 8).
- `fh check <file.inp>` parses and checks a model. It prints a TOON summary (DESIGN.md section 6.1).
- `fh results` prints TOON summaries. `fh plot` writes images (DESIGN.md section 8).
- M0: `fh --help` works (DESIGN.md section 13). The exact help text is not yet decided.
- Output format for each command beyond M0: not yet decided.

## Invariants the code must keep
- The binary is `fh`. Never `fea` (CLAUDE.md).
- Each command works over SSH and tmux, with no GUI (DESIGN.md section 3).
- The line budget is 3k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Name the binary `fea`.
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).
- Require a GUI for any command (DESIGN.md section 3).

## Validation hooks it must pass
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).
- Command-level tests: not yet decided.

## Examples
- `fh --help` prints the usage (DESIGN.md section 13, M0).
- `fh check <file.inp>` checks a model and prints a TOON summary (DESIGN.md section 6.1).
