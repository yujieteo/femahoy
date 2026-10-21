# femahoy: Product Design

This file holds the current rules of the product. Each rule has its reason. The rules for the agents that build femahoy are in `AGENTS.md`.

A rule stays only if it passes the test in D-30. Git keeps the old records.

---

## 1. Product

femahoy is a finite element and CFD workflow in Rust. LLM agents and humans use it together.

- The **engine** is the `fh` command-line program.
- The **main interface** is a visual chat, like ChatGPT or Codex. A 3D viewport is beside the chat.
- The **chat is the agent**: 1 focused agent that plans and does the work.
- The **input** is CalculiX `.inp` text for structures, an OpenFOAM case folder for CFD, a GetDP case folder for electromagnetics and acoustics, a topology case folder for topology optimization, and a Chrono case folder for multibody dynamics.
- **Agent-facing output** is plain text lines.
- **Results** are VTU files with a PVD collection, so ParaView can open them.
- The **solvers** are CalculiX (`ccx`) for structures, OpenFOAM for CFD, GetDP for electromagnetics and acoustics, and Project Chrono for multibody dynamics.

---

## 2. Separate programs

- CalculiX, gmsh, OpenFOAM and GetDP stay separate programs. femahoy never links or bundles them, because they are GPL.
- Project Chrono is BSD-3-Clause, but it also stays a separate program: a Python with PyChrono. Thus femahoy has no dependency on it.

---

## 3. Interfaces

| Interface | Purpose |
|---|---|
| Chat | Main interface, in a panel on the left of the page. One conversation with the agent. Every agent action shows as the command or the tool that it used. The text box is always available: a message during the work reaches the agent at its next step, and `Stop` ends the work at once. A status shows the current action of the agent and the time. |
| 3D viewport | In the center of the page. Mesh with element outlines, deformed shape with a scale factor, contour plot of one field, step selector. |

---

## 4. Architecture

```
 browser (page + wasm viewer)
        |  HTTP (127.0.0.1, token)
        v
 fh serve (daemon) --- starts ---> agent (Claude Code)
        |                                  |
      reads                                | runs fh commands
        |                                  v
   state folder <---- writes ----- fh run ----> ccx, OpenFOAM, GetDP, Chrono
 (previews, runs, results)
```

### 4.1 Daemon (`fh serve`)

- Owns the agent process. It reads the preview and run folders that `fh run` writes, and serves them to the page. It does not start `ccx`.
- Binds to 127.0.0.1 only.
- Makes a random token at start, as Jupyter does. Every HTTP request must carry the token.
- Rejects any request whose `Origin` is not the daemon's own page.
- Serves the page and the wasm viewer itself. On macOS it then opens the user's browser at that page. On other systems it only prints the address.
- The browser never holds an API key.

Reason: a web page on another origin must not use the daemon, and a key in the browser can leak.

### 4.2 Clients

- **Page:** the only UI. HTML and CSS with a small plain script, served by the daemon, with no build tool and no file from the network. It holds the chat and the list of the runs, in the Inter font.
- **wasm viewer:** the 3D viewport in a canvas of the page, built with `eframe` (egui with wgpu). egui draws only the viewport and its controls.
- **Report:** `fh report` writes 1 HTML file with the text of the agent, the same wasm viewer and the results of the runs that the agent names. The file opens with a double-click and needs no daemon and no network. The agent writes no PDF, image or viewer code.

Reason: a chat needs normal text: scrolling, selection and copy. The 3D viewport needs the GPU, and egui hosts it well.

### 4.3 Agent

- The agent is a CLI harness process: Claude Code, with the user's own Claude Code login. femahoy has no agent loop of its own.
- The agent runs under the permission system of its harness.

---

## 5. Input and meshes

- CalculiX `.inp` is the input format for structures. CalculiX reads and checks it. The small reader of `fh` is for the preview only (`docs/reference/inp.md`).
- An OpenFOAM case folder is the input for CFD. OpenFOAM reads and checks it.
- A GetDP case folder (`case.pro`, and `case.geo` or `case.msh`) is the input for electromagnetics and acoustics. gmsh meshes `case.geo`, and GetDP reads and checks the case.
- A topology case folder (`job.inp` and `topology.toml`) is the input for topology optimization. `job.inp` is CalculiX input with no material. `fh` adds 1 material for each element, and CalculiX reads and checks the input.
- A Chrono case folder (`chrono.py`, a PyChrono script) is the input for multibody dynamics. Python runs the script.
- `fh mesh` calls gmsh as an external program and writes the mesh and the named sets as CalculiX input. Users install gmsh themselves.
- The `.msh` reader takes only `C3D10` (type 11) volumes, or with `--shell` only `S6` (type 9) surfaces. Another type is an error.
- Text-to-CAD uses a Python script with build123d. The script writes STEP, and gmsh reads STEP with OpenCASCADE. `fh` has no CAD kernel. It reads only the length unit of a STEP file.

Reason of the first 4 rules: each solver reads only its own input, and `fh` gives the input to the solver as it is.

Reason of the last rule: Python CAD libraries exist, a chat can write the script, and gmsh already includes OpenCASCADE.

### 5.1 Units

`fh mesh` takes the units from the STEP unit record: SI-mm for mm, SI-m for m. It declares them in the line `** units: SI-mm` (or `SI-m`). It refuses to mesh when a STEP file has no unit record, mixed units, or a unit other than mm or m.

Reason: a wrong unit gives wrong results without any warning. There is no default unit and no guess.

### 5.2 Element

`C3D10` (tet10) is the element that `fh mesh` writes. With `--shell` it writes `S6` (the 6-node shell triangle) for a part drawn as a mid-surface. The chat can write another element type in its CalculiX input, for example `S8R`, and the page shows each element type that CalculiX writes to the result file.

Reason: tetrahedra mesh any shape, and quadratic elements do not have the bending errors of plain hex8. In the old `cantilever` case, plain hex8 (`C3D8`) gave 0.35 to 2.07 times the beam deflection. One element type keeps `fh mesh` and the cases small. `S6`, not `S8R`, is the shell element: on the roof case `S6` gives -301.5 mm on the 1250 mm mesh (reference -302.4 mm), but gmsh gives no reliable all-quad mesh of a curved surface (mixed meshes, or an element with 2 nodes at one point).

---

## 6. Solvers

`fh run` and `fh validate` start `ccx` directly, as a separate program that the user installs. The module `ccx` of the crate `fh` gives the input to `ccx` as it is, runs `ccx` and writes the results. `fh run` needs no daemon: the daemon and `fh run` share only the state folder. Each solve and `fh validate` stop with an error that tells how to install `ccx` when it is not there.

OpenFOAM is the CFD solver. It is a separate program that the user installs, as section 2 says.

Reason: a real problem needs CFD: a basic airfoil with control surfaces. CalculiX stays the structural solver, because no structural run needed another solver.

GetDP is the solver for electromagnetics and acoustics. It is a separate program that the user installs, as section 2 says.

Reason: CalculiX and OpenFOAM do not solve electromagnetic problems or the Helmholtz equation of acoustics. GetDP reads the meshes of gmsh, and its binaries run on macOS and Linux.

Topology optimization is a SIMP loop in `fh` around `ccx` (`crates/fh/src/topology.rs`). Each iteration solves the model with `ccx`, reads the strain energy of each element and changes the densities.

Reason: the design variables of CalculiX are node coordinates and orientations, not densities. The loop of Andreassen et al. (2011) is short, and `ccx` gives all the values that it needs.

Project Chrono is the multibody solver. It is a separate program that the user installs, as section 2 says.

Reason: the other solvers do not solve mechanisms with joints and large rigid motion, for example the actuator linkage of a control surface. PyChrono has builds for macOS (Apple Silicon) and Linux.

CalculiX (`*DYNAMIC, EXPLICIT`) is also the explicit solver for impact.

Reason: OpenRadioss, now OpenCourant, has builds for Linux and Windows only, not for macOS. `ccx` already runs on macOS and in CI.

### 6.1 Limits of the `ccx` process

- Each run has its own folder in the state folder, with the copy of the input, the `ccx` files and the results.
- Each run has a memory limit. The default is 85 percent of the RAM of the machine. A run has a time limit only when `--timeout` sets one.
- A cancel, the memory limit and the time limit kill the whole process group of `ccx` at once. A guard process kills the group when `fh` stops. When `fh run` ends for any reason, `ccx` stops: Ctrl+C, SIGTERM and SIGHUP cancel the run, and the guard process handles SIGKILL.

Reason: a large model can use all the RAM of the machine. A stop with a clear error is better than a machine that stops.

### 6.2 Results

- The run folder has `step-N.vtu` (the last increment of step N) for each step, the PVD collection, and the `ccx` files in `ccx/`.
- VTU: binary data appended raw. Sets are integer arrays.
- Atomic write: a writer writes a temporary file and renames it when complete. A reader never sees a partial file, because a run can die at any time.

---

## 7. Validation

- References are analytical solutions, measured values, or published numerical benchmark values. The cases are in `validation/cases/<id>/`.
- Cases use our own input files. Each target value cites its source.
- Each case has a fixed tolerance, written and justified in its files (`docs/reference/check-toml.md`).
- A structural case has 1 mesh. A CFD, electromagnetic or acoustic case has at least 2 meshes (3 where practical) and states the mesh error. A multibody case states the error of its time step from at least 2 time steps.
- A change to a solver or its adapter merges only if `fh validate` passes the full suite. A failure blocks the merge. There is no override.

Reason: a solver can give a wrong result without a warning. The suite is the only check. In CFD the mesh error is often the largest error. In the electromagnetic case it is 40 % of the tolerance. 1 mesh cannot show it.

---

## 8. Font

The Inter font is under the SIL Open Font License 1.1, in `crates/viewer/fonts` with its licence.

---

## 9. Decision log

Each entry is a rule that is in force. The owner decided each of them.

### D-30 A requirement stays only if a real run showed the need
A requirement stays only if a real run showed the need, it gives the user a result, and you can measure it. A rule that prevents a security fault, a licence or trademark fault, or a loss of data also stays. Its reason names that fault. Reverse for one item if a real run shows the need. Then add it as a new item with an exit test.
