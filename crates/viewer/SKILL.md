# viewer skill

## Purpose

The crate is a placeholder. `src/lib.rs` contains no implementation.

## Inputs and outputs

No viewport, terminal widget, chat panel or daemon client exists in this crate.

## Invariants the code must keep

`fh serve` serves a placeholder page. It does not use this crate yet.
- Line budget: 15k lines, tests included (DESIGN.md section 11).
- Wrap `alacritty_terminal` behind our own interface and pin an exact version (DESIGN.md section 11.1).

## Forbidden actions

- Do not describe these features as available.
- Hold an API key in the browser (DESIGN.md section 4.1).
- Bundle herdr (DESIGN.md section 4.3).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-viewer` and `cargo test -p fh-viewer`.

## Examples

Run `fh --help` to check the available commands.
