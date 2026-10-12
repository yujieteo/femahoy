# daemon skill

## Purpose
- The `fh serve` daemon: HTTP and WebSocket server, token auth, PTY, pane backends, process supervision (DESIGN.md sections 4.1 and 11).
- Owns solver runs, the PTY, the pane backend, agent processes, API keys and project files (DESIGN.md section 4.1).
- Licence role: app. Line budget: 7k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: HTTP and WebSocket requests from clients on 127.0.0.1 (DESIGN.md section 4.1).
- Output: the wasm UI page, served by the daemon. The daemon opens the user's browser at that page (DESIGN.md section 4.1).
- Output: pane sessions through `PaneBackend` (DESIGN.md section 4.3).

## Invariants the code must keep
- Bind to 127.0.0.1 only (DESIGN.md section 4.1).
- Make a random token at start. Every HTTP and WebSocket request must carry it (DESIGN.md section 4.1).
- Reject any request whose `Origin` is not the daemon's own page (DESIGN.md section 4.1).
- The browser never holds an API key (DESIGN.md section 4.1).
- `PaneBackend` has two implementations: herdr, the default when installed, and plain PTY, used when herdr is not installed (DESIGN.md section 4.3).
- The teammate tree reads from `PaneBackend`, never from herdr directly (DESIGN.md section 4.3).
- Line budget: 7k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Bind to any address other than 127.0.0.1 (DESIGN.md section 4.1).
- Bundle herdr. femahoy never bundles herdr (DESIGN.md section 4.3).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- M3 done-when: the viewer shows the M2 results in a browser, and herdr runs in the sidebar (DESIGN.md section 13).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- `fh serve` starts the daemon and opens the browser (DESIGN.md sections 4.1 and 13).
