# viewer skill

## Purpose

The crate has the result viewer (`docs/viewer.md`): one eframe app for the native window (`fh view`) and for the browser (wasm, the page of `fh serve`). The terminal widget, the chat panel and the teammate tree are not built.

## Inputs and outputs

- Input: an `.inp` model (mesh only), or a run folder with `results.pvd` and `step-N.vtu`. The browser gets the same files from the daemon (`web.rs`).
- Output: a 3D view with the mesh, the outlines, the deformed shape, one contour field and a step selector.

## Invariants the code must keep

- `mesh`, `results`, `view` and `camera` have no GPU or file types, so their tests run with no window.
- The contour values and the deformed positions come from the result files with no change other than `x + scale * U`.
- The browser sends the token only in the `Authorization` header, to its own origin.
- Line budget: 12k lines, tests included (DESIGN.md section 11).
- Wrap `alacritty_terminal` behind our own interface and pin an exact version (DESIGN.md section 11.1).

## Forbidden actions

- Do not describe the terminal widget, the chat panel or the teammate tree as available.
- Hold an API key in the browser (DESIGN.md section 4.1).
- Bundle herdr (DESIGN.md section 4.3).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-viewer` and `cargo test -p fh-viewer`. Run `scripts/build-viewer.sh` to check the wasm build.

## Examples

Run `fh --help` to check the available commands.
