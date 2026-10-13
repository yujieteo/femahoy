# daemon skill

## Purpose

The crate is a placeholder. `src/lib.rs` contains no implementation.

## Inputs and outputs

No HTTP server, WebSocket server, token authentication or pane backend exists in this crate.

## Invariants the code must keep

`fh serve` is not implemented in `cli/src/main.rs`.
- Line budget: 7k lines, tests included (DESIGN.md section 11).
- Bind to 127.0.0.1 only. Every request carries the random token and the daemon's own `Origin` (DESIGN.md section 4.1).
- The browser never holds an API key (DESIGN.md section 4.1).

## Forbidden actions

- Do not describe these features as available.
- Bind to any address other than 127.0.0.1 (DESIGN.md section 4.1).
- Bundle herdr (DESIGN.md section 4.3).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-daemon` and `cargo test -p fh-daemon`.

## Examples

Run `fh --help` to check the available commands.
