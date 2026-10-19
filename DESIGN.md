# femahoy: Product Design

This file holds the current rules of the product. Each rule has its reason. The rules for the agents that build femahoy are in `AGENTS.md`.

A rule stays only if it passes the test in D-30. Git keeps the old records.

---

## 1. Product

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

- The **engine** is the `fh` command-line program.
- The **main interface** is a visual chat, like ChatGPT or Codex. A 3D viewport is beside the chat.
- The **chat is the leader**: 1 focused agent that plans and does the work.
- The **input** is CalculiX `.inp` text.
- **Agent-facing output** is plain text lines.
- **Results** are VTU files with a PVD collection, so ParaView can open them.
- The **solver** is CalculiX (`ccx`).

---

## 2. Licence

- **Licence:** AGPL-3.0 for all code.
- CalculiX and gmsh stay separate programs. femahoy never links or bundles them, because they are GPL.

---

## 3. Interfaces

| Interface | Purpose |
|---|---|
| Chat | Main interface, in a panel on the left of the page. One conversation with the leader. Every leader action shows as the `fh` command it ran. The text box is always available: a message during the work reaches the leader at its next step, and `Stop` ends the work at once. A status shows the current action, the solve and its progress, the time, and each failure. |
| 3D viewport | In the center of the page. Mesh with element outlines, deformed shape with a scale factor, contour plot of one field, step selector. |
| `fh` CLI | Works alone over SSH and tmux, with no GUI. |

---

## 4. Architecture

```
 browser (page + wasm viewer)                       fh CLI (SSH / tmux)
        \                                                /
         \-------- HTTP (127.0.0.1, token) -------------/
                                |
                         fh serve (daemon)
             +-----------+-----------+-----------+
             |  solver   |  agent    |  files    |
             | (ccx)     | (Claude   | (project, |
             |           |  Code)    |  results) |
             +-----------+-----------+-----------+
```

### 4.1 Daemon (`fh serve`)

- Owns the solver runs, the agent process, and the project files.
- Binds to 127.0.0.1 only.
- Makes a random token at start, as Jupyter does. Every HTTP request must carry the token.
- Rejects any request whose `Origin` is not the daemon's own page.
- Serves the page and the wasm viewer itself, then opens the user's browser at that page.
- The browser never holds an API key.

Reason: a web page on another origin must not use the daemon, and a key in the browser can leak.

### 4.2 Clients

- **Page:** the only UI. HTML and CSS with a small plain script, served by the daemon, with no build tool and no file from the network. It holds the chat and the list of the runs, in the Inter font.
- **wasm viewer:** the 3D viewport in a canvas of the page, built with `eframe` (egui with wgpu). egui draws only the viewport and its controls.

Reason: a chat needs normal text: scrolling, selection and copy. The 3D viewport needs the GPU, and egui hosts it well.

### 4.3 Agent

- The agent is a CLI harness process: Claude Code, with the user's own Claude Code login. femahoy has no agent loop of its own.
- The leader runs under the permission system of its harness.

---

## 5. Input and meshes

- `.inp` is the native input format. One model format keeps the parser small. CalculiX reads and checks it. The small reader of `fh` is for the preview only (`docs/reference/inp.md`).
- `fh mesh` calls gmsh as an external program and writes the mesh and the named sets as CalculiX input. Users install gmsh themselves.
- The `.msh` reader takes only `C3D10` (type 11) volumes, or with `--shell` only `S6` (type 9) surfaces. Another type is an error.
- Text-to-CAD uses a Python script with build123d. The script writes STEP, and gmsh reads STEP with OpenCASCADE. `fh` has no CAD kernel and no STEP reader.

Reason of the last rule: Python CAD libraries exist, a chat can write the script, and gmsh already includes OpenCASCADE.

### 5.1 Units

`fh mesh` declares the units in the line `** units: SI-mm` (or `SI-m`). It refuses to mesh when `--units` is missing, when a STEP file has no unit record, and when the unit of the file does not match `--units`.

Reason: a wrong unit gives wrong results without any warning. There is no default unit and no guess.

### 5.2 Element

`C3D10` (tet10) is the element that `fh mesh` writes. With `--shell` it writes `S6` (the 6-node shell triangle) for a part drawn as a mid-surface. The chat can write another element type in its CalculiX input, for example `S8R`, and the page shows each element type that CalculiX writes to the result file.

Reason: tetrahedra mesh any shape, and quadratic elements do not have the bending errors of plain hex8. In the old `cantilever` case, plain hex8 (`C3D8`) gave 0.35 to 2.07 times the beam deflection. One element type keeps `fh mesh` and the cases small. `S6`, not `S8R`, is the shell element: on the roof case `S6` gives -301.5 mm on the 1250 mm mesh (reference -302.4 mm), but gmsh gives no reliable all-quad mesh of a curved surface (mixed meshes, or an element with 2 nodes at one point).

---

## 6. Solver

`fh` starts `ccx` directly, as a separate program that the user installs. The crate `solver-ccx` writes the `ccx` input, runs `ccx` and writes the results. Each solve and `fh validate` stop with an error that tells how to install `ccx` when it is not there.

Reason: its input is closest to `.inp`, and no run needed another solver.

### 6.1 Limits of the `ccx` process

- Each run has its own working folder.
- Each run has a memory limit. The default is 85 percent of the RAM of the machine. A run has a time limit only when `--timeout` or `fh validate` sets one.
- A cancel, the memory limit and the time limit kill the whole process group of `ccx` at once. A guard process kills the group when `fh` stops.

Reason: a large model can use all the RAM of the machine. A stop with a clear error is better than a machine that stops.

### 6.2 Results

- The run folder has `step-N.vtu` (the last increment of step N) for each step, the PVD collection, and the `ccx` files in `ccx/`.
- VTU: binary data appended raw. Sets are integer arrays.
- Atomic write: a writer writes a temporary file and renames it when complete. A reader never sees a partial file, because a run can die at any time.

---

## 7. Validation

- References are analytical solutions and measured values. The cases are in `validation/cases/<id>/`.
- No Abaqus licence is available, so there are no Abaqus runs. The Abaqus verification manual can be a list of problem ideas, cited by number.
- Cases use our own input files. Each target value cites its source.
- Each case has 1 mesh and a fixed tolerance, written and justified in its files (`docs/reference/check-toml.md`).
- A change to a solver or its adapter merges only if `fh validate` passes the full suite. A failure blocks the merge. There is no override.

Reason: a solver can give a wrong result without a warning. The suite is the only check.

---

## 8. Font

The Inter font is under the SIL Open Font License 1.1, in `crates/viewer/fonts` with its licence.

---

## 9. Decision log

Each entry is a rule that is in force. The captain decided each of them.

### D-30 A requirement stays only if a real run showed the need
A requirement stays only if a real run showed the need, it gives the user a result, and you can measure it. Reverse for one item if a real run shows the need. Then add it as a new item with an exit test.

### D-32 The docs set
The owner approved the docs set: `README.md`, `crates/daemon/src/agent.md`, `docs/reference` and this file.
