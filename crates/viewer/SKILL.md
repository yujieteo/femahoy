# viewer skill

## Purpose

The crate has the result viewer (`docs/viewer.md`): one eframe app for the native window (`fh view`) and for the browser (wasm, the page of `fh serve`). In the browser, the page also has the terminal sidebar (`docs/terminal.md`): `terminal.rs` is the terminal widget and `sidebar.rs` holds the tabs and the panes. `chat.rs` is the chat panel (`docs/chat.md`): the request, each action of the agent, and the results of a new run in the viewer. The teammate tree is not built.

## Inputs and outputs

- Input: an `.inp` model (mesh only), or a run folder with `results.pvd` and `step-N.vtu`. The browser gets the same files from the daemon (`web.rs`).
- Output: a 3D view with the mesh, the outlines, the deformed shape, one contour field and a step selector.
- The terminal widget reads the screens of the daemon in the protocol `fh.term.v1` and sends the keys, the paste, the mouse and the size. The sidebar reads and changes the terminals and the panes through the `Remote` trait.

## Invariants the code must keep

- `mesh`, `results`, `view`, `camera`, `terminal` and `sidebar` have no GPU or file types, so their tests run with no window.
- The contour values and the deformed positions come from the result files with no change other than `x + scale * U`.
- The browser sends the token only to its own origin: in the `Authorization` header, and in the WebSocket subprotocol `fh.token.<token>`.
- The viewer does not use `alacritty_terminal`. The daemon runs the terminal emulator (`crates/daemon/src/pty.rs`) and the viewer only draws its screens.
- The sidebar reads the panes from the daemon (DESIGN.md section 4.3).

## Forbidden actions

- Do not describe the teammate tree as available.
- Hold an API key in the browser (DESIGN.md section 4.1).

## Validation hooks it must pass

Run `cargo check -p fh-viewer` and `cargo test -p fh-viewer`. Run `scripts/build-viewer.sh` to check the wasm build.

## Examples

Run `fh --help` to check the available commands.
