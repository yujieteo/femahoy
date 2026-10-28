# Validation cases as 1 HTML file

`scripts/build-quine.sh` makes 1 HTML file for each of 5 validation cases. Each file holds the case, the native solvers of femahoy and the check of the case. Open the file from `file://` with no network and no install. The case runs at once, and the terminal shows `pass` or `fail` against the tolerance of `check.toml`.

[quine-wasm](https://github.com/yujieteo/quine-wasm) (MIT licence) makes the files. The script clones it at a fixed commit into the cargo target folder, and femahoy holds none of its code.

Sources: [program](../../crates/quine/src/lib.rs),
[native solve](../../crates/fh/src/validation/measure.rs),
[build script](../../scripts/build-quine.sh) and
[browser test](../../scripts/quine/quine.spec.js).

## Build and test

```sh
nix shell nixpkgs#emscripten nixpkgs#nodejs -c scripts/build-quine.sh --test
```

The script builds the crate `fh-quine` for `wasm32-unknown-emscripten` as a static library, and quine-wasm links it with emcc. The files go to `target/quine/`, or to the folder that you give as the last argument. With `--test`, Playwright opens each file in Chromium. The test checks the pass on the screen, the check and the result tables in the `.tar` export, and the results in a saved copy.

## The cases

Each case needs only the native solvers, has an analytic reference and a tolerance budget in its `check.toml`, and solves in seconds.

| Case | Physics | Size of file | Run in Chromium | Run in WebKit |
|---|---|---|---|---|
| `grid-heated-bar` | steady heat conduction | 2.5 MB | 0.1 s | 0.1 s |
| `grid-kirsch-hole` | linear elastic stress, 10-node tetrahedra | 2.5 MB | 4.5 s | 2.1 s |
| `line-coax` | 2D transmission line (C, L, Z0) | 2.5 MB | 0.3 s | 0.4 s |
| `grid-waveguide-te10` | 3D frequency-domain electromagnetics | 2.5 MB | 1.0 s | 0.9 s |
| `grid-laminate-cell` | homogenization of a unit cell | 2.5 MB | 0.0 s | 0.0 s |

The times are from 2026-10-28 on an Apple M5 Mac (macOS 27), with emcc 6.0.9 and Playwright 1.63. Natively the same solves take 0.03 s to 0.7 s.

## Use of a file

- The program is `fh-quine CASE`. It writes the results to `results/` and the check table to `results/check.txt`.
- Click "Export files as .tar" to get the case and the results. The `.vtu` files open in ParaView.
- Edit `grid.toml` in the files panel, then click "Run again" to solve the changed case.
- Click Save to get a copy of the HTML file with the current files and results.

## Limits

- Only grid cases (`grid.toml`) run. A case that needs CalculiX, GetDP, OpenFOAM or another program is refused with a message that names the solver.
- The page has 1 thread and at most 4 GB of memory. Large 3D models and long transient runs are not suitable.
- The page shows no 3D view. The viewer of `fh serve` is a different wasm build (wgpu with wasm-bindgen) and does not link with emcc. Thus the results leave the page by the `.tar` export.
- The page cannot run `fh serve`, `fh report` or the agent. The crate `fh` builds for this target only without its default feature `server`.
