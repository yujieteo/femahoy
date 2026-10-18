# Viewer

The viewer shows the results of a run in 3D. `fh serve` serves the viewer as wasm. The page of the daemon is the viewer, with the chat on the left (roadmap items M3 and M4, `docs/chat.md`).

```
fh serve                    # the viewer in the browser: fh serve opens its page
fh run model.inp            # solve, then open the results of the run in the browser
```

## What the viewer shows

| Item | What you see |
|---|---|
| Mesh | The surface of the part (the faces that belong to one element only), with the element edges in grey. |
| Outline | The edges where the surface turns by more than 30 degrees, and the edges at an open surface (dark lines). |
| Deformed shape | The nodes move by the scale factor times the displacement `U`. A grey outline shows the undeformed shape. |
| Contour | One field in 9 colour bands from blue (lowest) to red (highest), with a legend that gives the band limits and the unit. |
| Step selector | One entry for each step of `results.pvd`, with its time and its file. The `<` and `>` buttons go to the previous and the next step. |

The viewer draws `C3D4`, `C3D8` (also `C3D8R` and `C3D8I`) and `C3D10` elements, and the VTK cell types 10, 12 and 24. It skips other element types and elements that name a missing node. The status bar shows the counts.

### Deformed shape

The scale starts at the automatic value: the largest displacement of all steps shows as 1/10 of the model size, rounded to 1, 2 or 5 times a power of 10. You can type a scale, click **Auto** to go back to the automatic value, or click **1x** for the true scale. The panel shows the largest displacement of the step, in the length unit of the run.

### Contour

The contour fields come from the float arrays of the step file:

| Array | Fields | Where |
|---|---|---|
| `U`, `RF` (and `UR` when the solver writes it) | the magnitude and the components 1, 2 and 3, for example `U magnitude` and `U3` | at the nodes |
| `S` | `S11`, `S22`, `S33`, `S12`, `S23`, `S13` (the order of the text results) | per element |
| `MISES` | `MISES` | per element |

A field at the nodes is interpolated linearly over each surface triangle before it is put in a band. A field per element has one value over the element. The bands split the range from the smallest to the largest value of the step in 9 equal parts. When the spread of the values is less than 1e-9 of their size, the field is constant: the legend shows one band and the value. For example, the uniform stress of the `bar-pressure` case shows as `constant 100.0`.

The first field is `MISES`, else `U magnitude`. The units of the legend come from the unit system of the run: `SI-mm` gives mm, N and MPa, and `SI-m` gives m, N and Pa. A run with no unit system shows no unit.

### Supports and loads

Before each solve from the chat, the viewer shows the input of the solve: the faces of each support and each load in a colour, 1 arrow for each load, and a label for each region, for example `FIXED: ux uy uz` or `P1: 2700 N, -y`.
The results show the same arrows and labels, with the outline of the faces of each region. The labels move with the deformed shape. An outline on the far side of the part is a thin line.
The panel lists each region with its label. A check box in the panel hides the labels and the arrows. See [the chat](chat.md#the-model-before-the-solve).

When a later step has a load on the same set, its value replaces the value of the earlier step. The label gives the value of the last step.

### Controls

| Action | Input |
|---|---|
| Orbit | drag with the left button |
| Pan | drag with the right or middle button, or shift and left button |
| Zoom | scroll |
| Reset the view | double click |

## Result files

The viewer reads `results.pvd` and the `step-N.vtu` files that it names (`docs/quickstart.md`). Each step must have the mesh of the first step and a displacement field `U` with 3 components. The viewer refuses the results and shows the cause when a file is bad:

| Code | Cause |
|---|---|
| `VTU002`, `VTU003` | `results.pvd` or a VTU file is malformed or uses a feature that the reader does not support. |
| `results.empty` | `results.pvd` names no step file. |
| `results.file.path` | A step file name is not a plain file name in the run folder. It is empty, or it is `.` or `..`, or it has `/`, `\` or `:`. |
| `results.file.missing` | A step file cannot be read. |
| `results.mesh.empty` | A step file has no point or no cell. |
| `results.mesh.nonfinite` | A point coordinate is not finite (NaN or infinity). |
| `results.mesh.changed` | A step has a mesh that is not the mesh of the first step. |
| `results.mesh.unsupported` | The mesh has no cell that the viewer can draw. |
| `results.field.missing` | A step has no displacement field `U` with 3 float components. |
| `results.field.length` | A field has the wrong number of values. |
| `results.field.nonfinite` | A field has a value that is not finite (NaN or infinity). |

The message names the file, for example `step-2.vtu: not found (results.file.missing)`. In the browser, an HTTP error shows the path, the status and the code of the daemon, for example `HTTP 404: no run run-000099 in p-... (run.unknown)`.

## The viewer in the browser

1. Install `fh` with the viewer: `scripts/install.sh`. It builds the wasm viewer and copies it to the `viewer` folder beside `fh`.
2. Run `fh run model.inp` (or `fh run --example bar-pressure`). It solves the model and opens the browser on the run.
3. Or run `fh serve`. It opens the browser on the page of the daemon, with the list of the runs. Click a run in the list. The list shows each run that succeeded. To open a run directly, add `&project=P&run=R` to the address.

`--no-open`, or `FH_NO_OPEN=1`, keeps the browser closed. Then open the address that the command prints, `http://127.0.0.1:PORT/?token=TOKEN`.

To build only the viewer, for example in development, run `scripts/build-viewer.sh`. The script needs the Rust target `wasm32-unknown-unknown` and `wasm-bindgen-cli` at the version of `wasm-bindgen` in `Cargo.lock` (0.2.129). It writes `fh_viewer.js` and `fh_viewer_bg.wasm` to `release/viewer` in the cargo target folder (`$CARGO_TARGET_DIR` or `target`), the folder beside `fh`. `fh serve` finds the viewer in `$FH_VIEWER`, else in the `viewer` folder beside `fh`.

The page holds no token and no result. Its script reads the token from the page address. Then it fetches the viewer files and the result files with `Authorization: Bearer TOKEN`, so the token and `Origin` rules of `fh serve` stay the same (`docs/run-contract.md`). The result files come from `GET /api/v1/projects/<p>/runs/<r>/files/<name>`.

The browser needs WebGPU or WebGL 2. When the viewer cannot start, the page shows the cause.

## Code

The code is in `crates/viewer`:

| File | Content |
|---|---|
| `mesh.rs` | surface, outline and bounding box, from an `.inp` model or a VTU grid |
| `results.rs` | the reader of the result files, the contour fields, the deformed positions and the bands |
| `view.rs` | the view state: the step, the scale, the field and the vertex data |
| `camera.rs` | the orbit camera |
| `gpu.rs`, `shader.wgsl` | the thin wgpu layer |
| `app.rs` | the eframe app of the page |
| `chat.rs` | the chat panel and its requests to the daemon, behind the `Remote` trait |
| `web.rs` | the browser entry point and the HTTP requests (wasm only) |

`mesh.rs`, `results.rs`, `view.rs`, `camera.rs` and `chat.rs` have no GPU types and have unit tests. The chat tests use a fake daemon behind `Remote`. The tests read `fixtures/results/cantilever-2step` (2 steps of `fh-solve` output) and compare each displacement and each stress value with the text results of the same run.
