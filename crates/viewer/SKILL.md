# viewer skill

## Purpose
- The eframe app: a wgpu mesh and contour renderer, the terminal widget, the chat panel and the teammate tree (DESIGN.md sections 3, 4.2 and 11).
- Licence role: app. Line budget: 15k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: results files, read through `io` (DESIGN.md section 8).
- Input: the daemon connection on 127.0.0.1 (DESIGN.md section 4.1).
- Output: the 3D viewport, the chat, the teammate tree and the terminal sidebar (DESIGN.md section 3).

## Invariants the code must keep
- The same eframe code runs in the browser (wasm) and natively (DESIGN.md section 4.2).
- The teammate tree reads from `PaneBackend`, never from herdr directly (DESIGN.md section 4.3).
- The terminal sidebar is a real shell. Herdr runs there unchanged (DESIGN.md section 3).
- The keyboard is the main input. The mouse is for the viewport and the custom panels (DESIGN.md section 3).
- Wrap `alacritty_terminal` behind our own interface. Pin an exact version (DESIGN.md section 11.1).
- Line budget: 15k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Hold an API key in the browser (DESIGN.md section 4.1).
- Bundle herdr (DESIGN.md section 4.3).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- M3 done-when: the viewer shows the M2 results in a browser, and herdr runs in the sidebar (DESIGN.md section 13).
- M10 and M11: the same tests as M3 and M4, on native macOS and Windows (DESIGN.md section 13).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- Not yet decided.
