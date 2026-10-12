# cli skill

## Purpose
- `fh` commands (DESIGN.md section 11, row `cli`). The binary is `fh`. Never `fea`.

## Inputs and outputs
- Input: command line arguments. M0 accepts `-h` and `--help` only.
- Output: usage text on stdout for help. Usage text and errors on stderr for unknown arguments, exit code 2.
- Licence role: app. Line budget: 3k, tests included.

## Invariants the code must keep
- `fh --help` prints a usage line and the planned subcommands (`check`, `validate`, `results`, `plot`, `serve`, `init`, `team`, `skills`) marked "not implemented", then exits 0 (DESIGN.md section 13, M0).
- An unknown argument prints usage to stderr and exits 2.
- Stay inside the line budget of 3k (DESIGN.md section 11).

## Forbidden actions
- Add a dependency without leader approval. M0 uses `std` only (AGENTS.md section 3, rule 4).
- Use the binary name `fea`. The binary is `fh` (CLAUDE.md).

## Validation hooks it must pass
- `cargo test -p fh-cli` (tests in `tests/cli.rs`).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-budget.sh` (AGENTS.md section 6).

## Examples
- `fh --help` prints usage and exits 0.
- `fh --bogus` prints usage to stderr and exits 2.
