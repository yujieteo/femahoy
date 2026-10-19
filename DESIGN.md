# femahoy: Product Design

This file holds the current rules of the product. Each rule has its reason. The rules for the agents that build femahoy are in `AGENTS.md`.

A rule stays only if it passes the D-30 test: a real run showed the need, it gives the user a result, and you can measure it. Git keeps the old records.

Code that shows a decision has a comment `Decision D-xx, see DESIGN.md`.

---

## 1. Product

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

- The **engine** is the `fh` command-line program.
- The **main interface** is a visual chat, like ChatGPT or Codex. A 3D viewport is beside the chat.
- The **chat is the leader**: 1 focused agent that plans and does the work.
- The **input** is CalculiX `.inp` text.
- **Agent-facing output** is plain text lines.
- **Results** are VTU files with a PVD collection, so ParaView can open them.
- The **solver** is CalculiX (`ccx`). `fh` starts it directly. Each validation case passes against its reference.

Description rule: say the product is "heavily inspired by Abaqus input conventions". Never use "Abaqus" as a product name or in a product name. Reason: Abaqus is a trademark of Dassault Systèmes.

---

## 2. Licence

- **Licence:** AGPL-3.0 for all code.
- CalculiX and gmsh stay separate programs. femahoy never links or bundles them, because they are GPL.

---

## 3. Interfaces

| Interface | Purpose |
|---|---|
| Chat | Main interface, in a panel on the left of the page. One conversation with the leader. Every leader action shows as the `fh` command it ran. The text box is always available: a message during the work reaches the leader at its next step. A status shows the current action, the solve and its progress, and each failure. |
| 3D viewport | In the center of the page. Mesh with element outlines, deformed shape with a scale factor, contour plot of one field, step selector. |
| `fh` CLI | Works alone over SSH and tmux, with no GUI. |

Rule: the keyboard is the main input. The mouse is for the viewport.

Rule: the page shows only what helps the work: the chat, the run in the viewport and its controls (D-28).

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

- **Page:** the only UI. HTML and CSS with a small plain script, served by the daemon, with no build tool and no file from the network. It holds the chat and the list of the runs, in the Inter font (D-28).
- **wasm viewer:** the 3D viewport in a canvas of the page, built with `eframe` (egui with wgpu). egui draws only the viewport and its controls. There is no native window, so the `fh` binary does not link eframe and wgpu.
- **`fh` CLI:** `fh run` uses the daemon, and starts one when none answers. `fh validate` and `fh mesh` run alone.

### 4.3 Agent

- The agent is a CLI harness process: Claude Code, with the user's own Claude Code login. femahoy has no agent loop of its own (D-27).
- The leader runs in headless mode with a JSON event stream (`claude -p --output-format stream-json`). The leader process stays alive between the messages, and each message goes to its stdin.
- The leader runs under the permission system of its harness.
- The daemon has no pane backend and no terminal sidebar (D-27).

---

## 5. Input and meshes

- `.inp` is the native input format. CalculiX reads and checks it. The small reader of `fh` takes only the keywords for the preview (`docs/reference/inp.md`) and never refuses a keyword.
- `fh mesh` calls gmsh as an external program and writes the nodes, the `C3D10` elements (the `S6` elements with `--shell`) and the named sets as CalculiX input. Users install gmsh themselves (D-02).
- The `.msh` reader takes only `C3D10` (type 11) volumes, or with `--shell` only `S6` (type 9) surfaces. Another type is an error.
- Text-to-CAD uses a Python script with build123d. The script writes STEP, and gmsh reads STEP with OpenCASCADE. `fh` has no CAD kernel and no STEP reader (D-17).

Reason of the last rule: Python CAD libraries exist, a chat can write the script, and gmsh already includes OpenCASCADE.

### 5.1 Units (D-18)

`fh mesh` declares the units in the line `** units: SI-mm` (or `SI-m`). It refuses to mesh when `--units` is missing, when a STEP file has no unit record, and when the unit of the file does not match `--units`.

Reason: a wrong unit gives wrong results without any warning. There is no default unit and no guess.

### 5.2 Element (D-19)

`C3D10` (tet10) is the element that `fh mesh` writes. With `--shell` it writes `S6` (the 6-node shell triangle) for a part drawn as a mid-surface. The chat can write another element type in its CalculiX input, for example `S8R`, and the page shows each element type that CalculiX writes to the result file.

Reason: tetrahedra mesh any shape, and quadratic elements do not have the bending errors of plain hex8. In the old `cantilever` case, plain hex8 (`C3D8`) gave 0.35 to 2.07 times the beam deflection. One element type keeps `fh mesh` and the cases small. `S6`, not `S8R`, is the shell element: on the roof case `S6` gives -301.5 mm (reference -302.4 mm) at every mesh size, but gmsh gives no reliable all-quad mesh of a curved surface (mixed meshes, or an element with 2 nodes at one point).

---

## 6. Solver

`fh` starts `ccx` directly, as a separate program that the user installs (D-13, D-31). The crate `solver-ccx` writes the `ccx` input, runs `ccx` and writes the results. Each solve and `fh validate` stop with an error that tells how to install `ccx` when it is not there.

The owner removed the native solver `fh-solve` on 2026-10-19 (D-29). An own solver is a main advantage later, but not now. Main commit `08a97c2` has its code.

### 6.1 Limits of the `ccx` process

- Each run has its own working folder.
- Each run has a memory limit. The default is 85 percent of the RAM of the machine. A time limit applies only when the case or an option sets one.
- A cancel, the memory limit and the time limit kill the whole process group of `ccx` at once. A guard process kills the group when `fh` stops.
- The run reads the memory use of the group with an operating system probe (Linux and macOS).
- The run folder has `step-N.vtu` for the last increment of step N, the PVD collection, and the `ccx` files in `ccx/`. There is no summary file. The run record has the state and the time.

Reason: a large model can use all the RAM of the machine (D-06). A stop with a clear error is better than a machine that stops.

### 6.2 Results

- The solver writes one VTU file for each step (`step-N.vtu`) and a PVD collection (D-04).
- VTU: binary data appended raw. Sets are integer arrays. femahoy writes and reads these files in Rust, with no C library. There is no compression, because that needs a new dependency and costs time on a slow CPU.
- Every writer takes one array at a time and makes no copy of a field.
- Atomic write (D-09): a writer writes a temporary file and renames it when complete. A reader never sees a partial file.
- Agents read plain text lines. Full field data stays in the VTU files.

---

## 7. Validation

- References are analytical solutions and measured values. The cases are in `validation/cases/<id>/`.
- No Abaqus licence is available, so there are no Abaqus runs. The Abaqus verification manual can be a list of problem ideas, cited by number. Never copy its text, `.inp` files, figures, or numbers into the repo.
- Each case is a data file pair: `job.inp` and `check.toml` (D-01). The fields are in `docs/reference/check-toml.md`.
- Cases use our own input files. Each target value cites its source.
- Each case has 1 mesh and a fixed tolerance, written and justified in its files. A quantity passes if `|computed - reference| <= rel_tol * |reference| + abs_tol`.
- `fh validate` runs the suite and reports pass or fail per case. A change to a solver or its adapter merges only if the full suite passes. A failure blocks the merge. There is no override.

Reason: a solver can give a wrong result without a warning. The suite is the only check.

---

## 8. Workspace and machine

- Cargo workspace. Keep the code small. Delete code that is not needed, and prefer a deletion to an addition (D-23).
- CalculiX and gmsh are external programs and are never linked. The Inter font is under the SIL Open Font License 1.1, in `crates/viewer/fonts` with its licence.
- **Target machine** (D-06): 16 GB RAM, 4 to 6 cores, a hard disk or a slow SSD. Input files are small. Results files are not: write them in pieces.
- **Platforms:** macOS and Linux. The user installs `fh` with `scripts/install.sh`, runs `fh serve`, and the UI opens in the browser.
- **aarch64** (D-07): all code builds and passes tests on aarch64 with default flags. The captain tests on an Apple Silicon MacBook, and CI builds and tests on aarch64 macOS.

---

## 9. Decision log

Each entry is a rule that is in force. The captain decided each of them.

### D-01 The check file of a case is TOML
`check.toml` is easy for people and agents to read and edit, and it has comments. TOML holds case metadata only, such as references and tolerances. The model is a `.inp` file (D-05).

### D-02 gmsh stays external, `fh mesh` is approved
gmsh is GPL. A separate program keeps the licence of femahoy clean.

### D-04 Results are VTU with PVD
VTU needs no large library, and ParaView opens it. HDF5 and Exodus need C libraries that are hard to build.

### D-05 The model is a `.inp` file
One model format keeps the parser small. Reverse if a real run shows that a second format is needed.

### D-06 Target machine
16 GB of RAM, 4 to 6 cores and a slow disk are the machine of the expected user. The plate on a 10 mm mesh needs 7.5 GiB, and a 16 GB laptop solves it.

### D-07 Code builds and passes on aarch64
The captain tests on an Apple Silicon MacBook.

### D-09 No reader sees a partial file
A run on a slow disk can die at any time. Writers write to a temporary file and rename it.

### D-13 CalculiX is the solver, and `fh` starts it directly
CalculiX passed all 6 capability tests of the first check, and its input is closest to `.inp`. It is GPL, so it stays a separate program. Since D-31, `fh` calls the adapter crate in its own process.

### D-17 Text-to-CAD through Python, STEP through gmsh
See section 5.

### D-18 The mesh declares units, `fh` refuses a mismatch
See section 5.1. Reverse if the captain allows a run with unknown units.

### D-19 C3D10 is the default element
See section 5.2.

### D-23 Delete code that is not needed
The captain removed the line budget on 2026-10-18, because a line count does not measure quality. Prefer a deletion to an addition.

### D-27 One focused agent, no pane backend
The captain said on 2026-10-18: "1 focused agent focusing on the right thing with the option to add more agents is OK" and "Delete herdr, I just want to talk". The chat has 1 agent, and the daemon has no pane backend.

### D-28 An HTML page, with the chat at the side and the model in the center
The captain said on 2026-10-19: "It should look like ChatGPT somehow", "Use Inter", "Only the renderer requires the egui right?", "Chat should not be in the center. It should be at the side, with the FEM in the center." and "do not disable the textbook I should be able to interrupt the worker just like now".
1. The page is HTML and CSS with a small plain script, and no build tool or package manager. The chat is a panel on the left. The 3D viewport fills the center.
2. egui draws only the 3D viewport and its controls.
3. The font is Inter. The page loads no file from the network.
4. The text box of the chat is always available. A message during the work reaches the leader at its next step. `Stop` ends the work at once.
5. While the leader works, the chat shows the current action, the solve and its progress, the time, and each failure.

Reason: a chat needs normal text: scrolling, selection and copy. egui draws its text on a canvas, but the 3D viewport needs the GPU and egui hosts it well.

### D-29 The native solver is removed
The owner said on 2026-10-19: "It is a main advantage but not now, besides it is recorded in git?" Main commit `08a97c2` has the full native solver. Reverse if the owner gives an own solver a milestone.

### D-30 A requirement stays only if a real run showed the need
A requirement stays only if a real run showed the need, it gives the user a result, and you can measure it. The real runs are the chat cantilever, the owner's plate, the 2 warpage papers and the first user test. The owner said on 2026-10-19: "yeah these requirements are stupid delete them". Reverse for one item if a real run shows the need. Then add it as a new item with an exit test.

### D-31 `fh` starts `ccx` directly, and the run summary goes
The owner said on 2026-10-19: "yes do 1 2 and 3, replace toon" and then "Agree with all." The adapter is a library that `fh serve` and the `fh` command call in their own process. There is no `fh-solve-ccx` program, no protocol, no TOON and no summary file. Reason: no run needed another solver, the summary had no result numbers, and nobody measured the token saving of TOON. Reverse if a real run needs a second solver or a summary file.

### D-32 Cut the docs and the requirements that fail the D-30 test
The captain said on 2026-10-19: "yes do 1 2 and 3, replace toon", "are our requirements stupid?" and "Focus on removing dumb requirements first before we start (a)". The answer was "some are". The captain approved the docs set: `README.md`, `crates/daemon/src/agent.md`, `docs/reference` and this file. This file lost the milestones, the reference set, the native solver text, the NAFEMS legal check and the reversed records (D-03, D-12, D-15, D-16, D-20 to D-22, D-24 to D-26). The 7 crate `SKILL.md` files, the rules that required them and the gmsh tested-release warning are also gone. Reason: the old text was not checked against D-30.
