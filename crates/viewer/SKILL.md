# viewer skill

## Purpose

The crate has the result viewer (`docs/viewer.md`): one eframe app for the browser (wasm), in the center of the page of `fh serve`. The page itself, with the chat and the list of the runs, is HTML in `crates/daemon` (`src/page.html`). The page calls `start` and `Viewer::open` of `web.rs`. The teammate tree is not built.

## Inputs and outputs

- Input: a run folder with `results.pvd` and `step-N.vtu`. The browser gets the files from the daemon (`web.rs`).
- Output: a 3D view with the mesh, the outlines, the deformed shape, one contour field and a step selector.
- `font()` gives the Inter font (`fonts/`, SIL Open Font License 1.1) to the page, so the page and the viewer use 1 font file.

## Invariants the code must keep

- `mesh`, `results`, `view` and `camera` have no GPU or file types, so their tests run with no window.
- The contour values and the deformed positions come from the result files with no change other than `x + scale * U`.
- The browser sends the token only to its own origin, in the `Authorization` header.

## Forbidden actions

- Do not describe the teammate tree as available.
- Hold an API key in the browser (DESIGN.md section 4.1).

## Validation hooks it must pass

Run `cargo check -p fh-viewer` and `cargo test -p fh-viewer`. Run `scripts/build-viewer.sh` to check the wasm build.

## Examples

Run `fh --help` to check the available commands.
