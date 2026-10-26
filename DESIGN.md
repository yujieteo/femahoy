# femahoy: Product Design

This file holds the current rules of the product. Each rule has its reason. The rules for the agents that build femahoy are in `AGENTS.md`.

A rule stays only if it passes the test in D-30. Git keeps the old records.

---

## 1. Product

femahoy is a finite element and CFD workflow in Rust. LLM agents and humans use it together.

- The **engine** is the `fh` command-line program.
- The **main interface** is a visual chat, like ChatGPT or Codex. A 3D viewport is beside the chat.
- The **chat is the agent**: 1 focused agent that plans and does the work.
- The **analysis types**, their **solvers** and their **input** are in the table of section 6.
- **Agent-facing output** is plain text lines.
- **Results** are VTU files with a PVD collection, so ParaView can open them, and curves: CSV files of x-y values.

---

## 2. Separate programs

- gmsh and each solver of the table in section 6, with its adapters, stay separate programs. femahoy never links or bundles them, because most of them are GPL or LGPL.
- A solver under a permissive licence also stays a separate program, for example Project Chrono (BSD-3-Clause) as a Python with PyChrono. Thus femahoy has no dependency on a solver.

---

## 3. Interfaces

| Interface | Purpose |
|---|---|
| Chat | Main interface, in a panel on the left of the page. One conversation with the agent. Every agent action shows as the command or the tool that it used. The text box is always available: a message during the work reaches the agent at its next step, and `Stop agent and commands` ends the work at once. A status shows the current action of the agent and the time. |
| 3D viewport | In the center of the page. Mesh with element outlines, deformed shape with a scale factor, contour plot of one field, step selector that selects and plays each output time. The panel shows each control that applies to the analysis and its important fields, and hides the other fields behind 1 switch, "Show all controls". In a report, the agent chooses the controls and the view. |

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
   state folder <---- writes ----- fh run ----> solvers (section 6)
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

- **Page:** the only UI. HTML and CSS with a small plain script, served by the daemon, with no build tool and no file from the network. It holds the chat and the list of the runs, in the style of the site teoyujie.org: the Source Serif 4 and DM Sans fonts, and its colours.
- **wasm viewer:** the 3D viewport in a canvas of the page, built with `eframe` (egui with wgpu). egui draws only the viewport and its controls.
- **Report:** `fh report` writes 1 HTML file with the text of the agent, the same wasm viewer, the results of the runs and an x-y plot of each curve that the agent names. A report has no fixed form: the agent puts each plot and each control in its text, for the conclusion of the results. A control opens a run with the view that the agent gives: the fields, the viewer controls, the step, the scale, the box of the camera and the marks on the model. The agent also writes the controls of the 3D view in the bar above the viewer, for example a choice of the flap angle or a slider of the time, the phase or the iteration. The viewer shows each of its own controls that applies to the run, or only the controls that the view names in `data-controls`. The switch "Show all controls" shows the others. `fh` draws each plot as inline SVG, with the reference points of the curve over it. The file opens with a double-click and needs no daemon and no network. With `--viewer`, many reports in 1 folder share 1 copy of the viewer (`validation/reports`). The agent writes no PDF, image, plot or viewer code.

Reason: a chat needs normal text: scrolling, selection and copy. The 3D viewport needs the GPU, and egui hosts it well.

Reason of the plots: many analysis types give a curve, not a field, for example an S11 over the frequency, the front of a dam break over the time or a mesh study. The viewer shows only fields, so a report showed a curve only as a table. `fh` draws the plot, so the agent still writes no image code.

### 4.3 Agent

- The agent is a CLI harness process: Claude Code, with the user's own Claude Code login. femahoy has no agent loop of its own.
- At start, `fh serve` checks that the agent starts and that `claude auth status` gives a login. If not, the chat tells the person how to install Claude Code or how to sign in. femahoy reads no credential.
- The agent runs under the permission system of its harness.

Reason of the check: a missing or signed-out agent showed only as a failed chat.

---

## 5. Input and meshes

- The table of section 6 gives the input of each analysis type. The solver reads and checks its input. The small reader of `fh` for CalculiX `.inp` is for the preview only (`docs/reference/inp.md`).
- `fh mesh` calls gmsh as an external program and writes the mesh and the named sets as CalculiX input. `scripts/install.sh` installs gmsh with the solvers.
- The `.msh` reader takes only `C3D10` (type 11) volumes, or with `--shell` only `S6` (type 9) surfaces. Another type is an error.
- Text-to-CAD uses a Python script with build123d. The script writes STEP, and gmsh reads STEP with OpenCASCADE. `fh` has no CAD kernel. It reads only the length unit of a STEP file.

Reason of the first rule: each solver reads only its own input, and `fh` gives the input to the solver as it is. Only the topology loop adds to the input (section 6).

Reason of the last rule: Python CAD libraries exist, a chat can write the script, and gmsh already includes OpenCASCADE.

### 5.1 Units

`fh mesh` takes the units from the STEP unit record: SI-mm for mm, SI-m for m. It declares them in the line `** units: SI-mm` (or `SI-m`). It refuses to mesh when a STEP file has no unit record, mixed units, or a unit other than mm or m.

Reason: a wrong unit gives wrong results without any warning. There is no default unit and no guess.

### 5.2 Element

`C3D10` (tet10) is the element that `fh mesh` writes. With `--shell` it writes `S6` (the 6-node shell triangle) for a part drawn as a mid-surface. The chat can write another element type in its CalculiX input, for example `S8R`, and the page shows each element type that CalculiX writes to the result file.

Reason: tetrahedra mesh any shape, and quadratic elements do not have the bending errors of plain hex8. In the old `cantilever` case, plain hex8 (`C3D8`) gave 0.35 to 2.07 times the beam deflection. One element type keeps `fh mesh` and the cases small. `S6`, not `S8R`, is the shell element of `fh mesh --shell`: on the roof case `S6` gives -301.5 mm on the 1250 mm mesh (reference -302.4 mm), but gmsh gives no reliable all-quad mesh of a general curved surface (mixed meshes, or an element with 2 nodes at one point). A mapped `S8R` mesh works for a surface patch with 4 edges: the roof case uses a transfinite gmsh mesh of 1825 nodes and gives -301.9 mm. `S6` from `fh mesh --shell` stays for other curved surfaces, and the roof case also checks it.

---

## 6. Solvers

`fh run` and `fh validate` start each solver directly, as a separate program (section 2). `scripts/install.sh` installs each solver for the user, with no root. `fh run` needs no daemon: the daemon and `fh run` share only the state folder. Each solve stops with an error that tells how to install its solver when the solver is not there.

This table is the only list of the solvers in the docs. A new solver adds 1 row here, and the other docs point to this table. CI and the install script have the other lists of the solvers: `scripts/ci-select.sh`, `.github/workflows/ci.yml` and `scripts/install.sh` must install each tool, so they name each solver.

| Analysis type | Solver | Input | Reason |
|---|---|---|---|
| Structures | CalculiX (`ccx`). The module `ccx` of the crate `fh` gives the input to `ccx` as it is, runs `ccx` and writes the results. | CalculiX `.inp` text | No structural run needed another solver. |
| CFD | OpenFOAM | OpenFOAM case folder | A real problem needs CFD: a basic airfoil with control surfaces. |
| Electromagnetics, acoustics | GetDP. gmsh meshes `case.geo`. | GetDP case folder: `case.pro`, and `case.geo` or `case.msh` | CalculiX and OpenFOAM do not solve electromagnetic problems or the Helmholtz equation of acoustics. GetDP reads the meshes of gmsh, and its binaries run on macOS and Linux. |
| Fluid-structure interaction | preCICE couples OpenFOAM and `ccx_preCICE` (CalculiX with the preCICE adapter). `fh run` starts both at the same time. The OpenFOAM adapter is a library that OpenFOAM loads. | preCICE case folder: `precice-config.xml`, the OpenFOAM case `fluid` and the CalculiX input `solid/job.inp` | The flow on a soft part changes its shape, and the shape changes the flow. preCICE couples the 2 solvers that femahoy already uses, so femahoy needs no third solver. |
| Topology optimization | A SIMP loop in `fh` around `ccx` (`crates/fh/src/topology.rs`). Each iteration solves the model with `ccx`, reads the strain energy of each element and changes the densities. For 2D elements, `fh` then writes the solid of the last design as a STEP file with gmsh, and `ccx` checks the solid with the loads of the case. | Topology case folder: `job.inp`, CalculiX input with no material, and `topology.toml`. `fh` adds 1 material for each element. | The design variables of CalculiX are node coordinates and orientations, not densities. The loop of Andreassen et al. (2011) is short, and `ccx` gives all the values that it needs. A density field is not a part. The STEP file is the geometry that a CAD program can use. |
| Shape and parameter optimization | A Nelder-Mead loop in `fh` around gmsh and `ccx` (`crates/fh/src/optimize.rs`). The variables are the `DefineConstant` values of `mesh.geo`. Each design runs gmsh and `ccx`, and the objective is the largest value of a nodal field on a node set. | Shape optimization case folder: `optimize.toml`, `mesh.geo` and `job.inp`. gmsh meshes `mesh.geo` for each design, and CalculiX reads and checks `job.inp`. | A shape change needs a new mesh, and gmsh already makes the meshes of femahoy. The loop needs no gradient, so it works with each field that `ccx` writes. For 2 to 5 variables, it needs about 50 to 200 solves. |
| Random vibration | A short step in `fh` after `ccx` (`crates/fh/src/ccx/rms.rs`). `ccx` solves a `*STEADY STATE DYNAMICS` step with the square root of the PSD as the amplitude. `fh` integrates the squared response over the frequencies and writes the RMS to `ccx/rms.txt`. | CalculiX `.inp` text | `ccx` has no random response step. The integral needs only the response at each frequency, which `ccx` prints. |
| Multibody dynamics | Project Chrono: a Python with PyChrono runs the script | Chrono case folder: `chrono.py`, a PyChrono script | The other solvers do not solve mechanisms with joints and large rigid motion, for example the actuator linkage of a control surface. PyChrono has builds for macOS (Apple Silicon) and Linux. |
| Particles (SPH, DEM) | LAMMPS | LAMMPS case folder: `case.lmp` | The mesh solvers do not solve a free surface with a large motion or a granular flow. LAMMPS has packages for macOS (Apple Silicon) and Linux. DualSPHysics does not run on Apple Silicon. |
| Explicit dynamics (impact) | CalculiX (`ccx`) with `*DYNAMIC, EXPLICIT` | CalculiX `.inp` text | OpenRadioss, now OpenCourant, has builds for Linux and Windows only, not for macOS. `ccx` already runs on macOS and in CI. |
| Optics and photonics | Meep: a Python with Meep runs the script | Meep case folder: `meep_case.py`, a Meep script | An optics problem needs the spectrum of a structure of a few wavelengths in open space. GetDP solves 1 frequency in each run. Meep, an FDTD solver, gives the full spectrum from 1 run on a regular grid with no mesh, and it has builds for macOS (Apple Silicon) and Linux. |
| Antennas | openEMS (FDTD): a Python with the modules of openEMS runs the script | openEMS case folder: `openems.py`, a script for the Python modules of openEMS | GetDP solves 1 frequency in each solve. An antenna needs its input impedance and S11 over a broad band, and its far field. 1 FDTD run of openEMS gives all of them. openEMS builds from source on macOS (Apple Silicon) and Linux. |

Reason of the single table: the list of the solvers was in 6 places, and each new solver had to change all of them.

### 6.1 Limits of each solver process

These limits apply to each solver process that `fh run` starts, with the same mechanism (`supervise` in `crates/fh/src/ccx/mod.rs`).

- Each run has its own folder in the state folder, with the copy of the input, the files of the solver and the results.
- Each run has a memory limit of 85 percent of the RAM of the machine. When a run starts 2 solver processes at the same time (preCICE), each has half of the limit. A run has a time limit only when `--timeout` sets one. The time limit counts all the processes of a run.
- A cancel, the memory limit and the time limit kill the whole process group of the solver at once. A guard process kills the group when `fh` stops. When `fh run` ends for any reason, the solver stops: Ctrl+C, SIGTERM and SIGHUP cancel the run, and the guard process handles SIGKILL.

Reason: a large model can use all the RAM of the machine. A stop with a clear error is better than a machine that stops. One mechanism for all solvers keeps the limits the same for each solver.

### 6.2 Results

- The run folder has 1 VTU file `step-N.vtu` for each output time N of the solver, the PVD collection that lists each file with its time, and the files of the solver in its folder (for example `ccx/` or `foam/`). The input sets the output times: each increment and each mode that `ccx` writes to the `.frd` file (`FREQUENCY`), each write time of OpenFOAM (`writeInterval`), each step of the `.pos` views of GetDP and Chrono (for example each time), and each snapshot of the LAMMPS dump. ParaView plays the collection as an animation, and the step selector of the viewer selects and plays each output time.
- VTU: binary data appended raw.
- Atomic write: a writer writes a temporary file and renames it when complete. A reader never sees a partial file, because a run can die at any time.
- A curve is a CSV file of x-y values: the x and y quantities with their units, 1 or more series, and reference points with their source (`crates/fh/src/curve.rs`). A run, a script of the agent or the agent writes it.
- `fh fit` fits a model to a curve by least squares: a polynomial, a power law (the Paris law), and Neo-Hooke and Mooney-Rivlin in uniaxial tension. It prints the parameters, R^2, the largest residual, the range of the points and, for a hyperelastic model, the `*HYPERELASTIC` lines of CalculiX.

Reason of the fit: the parameters of a model often come from a test curve, for example the constants of a rubber or C and m of the Paris law. Each model is linear in its parameters, or in ln x and ln y, so the fit needs no start value and no new dependency. The `*HYPERELASTIC` lines use the same constants as `ccx`: a C3D8 element of `ccx` gives the stress of the fit.

Reason of the output times: the dam break, the Taylor bar impact and the supersonic wedge showed only their last state, because the run kept only the last increment of each `ccx` step and the last time of OpenFOAM. The input already sets how often the solver writes, so `fh run` has no option for it.

---

## 7. Validation

- References are analytical solutions, measured values, or published numerical benchmark values. The cases are in `validation/cases/<id>/`.
- Cases use our own input files. Each target value cites its source.
- Each case has a fixed tolerance, written and justified in its files (`docs/reference/check-toml.md`).
- Each case states its discretization error from at least 2 meshes or 2 time steps, or states why it cannot.
- A change to a solver or its adapter merges only if `fh validate` passes the cases of that solver. A failure blocks the merge. There is no override.
- On a pull request, CI installs only the solvers of the changed paths and runs their cases (`scripts/ci-select.sh`). A change to shared code outside 1 solver module, to the Cargo files or to CI selects every solver. After each merge, CI runs the full suite on main.

Reason: a solver can give a wrong result without a warning. The suite is the only check. In CFD the mesh error is often the largest error. In the electromagnetic case it is 40 % of the tolerance. 1 mesh cannot show it. 1 rule for all analysis types means that a new analysis type needs no new rule.

Reason of the selection: a change to 1 solver does not change the results of the other solvers, and the full suite on each pull request took about 12 minutes for 7 solver tools. The full suite on main finds a fault of the selection after 1 merge.

---

## 8. UI and font

Each view uses 1 UI layer: the page of `fh serve`, a report and the model view. `crates/viewer/src/ui.rs` holds the only design tokens: the colours of the light and the dark appearance, the type sizes, the spacing, the corner radius and the hit target. It gives them to the page and to a report as CSS, with the components of `ui.css`. The model view gets the same tokens and components in egui (`widgets.rs`). The components are the toolbar and its buttons, the segmented control, the menu (`select`), the legend, the value readout and the keyboard-shortcut help row.

- A new view uses these tokens and components. It adds no colour, size or control style of its own. A new component goes into the layer, in CSS and in egui.
- A legend names the field and its unit. When the source does not define the unit, it says "unit unspecified".
- The layer follows the Apple Human Interface Guidelines: a control that the person clicks is at least 44 points high, each HTML control has a keyboard path and a label for VoiceOver, a shortcut shows its keys, both appearances work, no animation runs with reduced motion, and no state shows by colour only. The egui controls of the model view take the keyboard focus, but egui gives no labels to VoiceOver on the web: the canvas has only its own label.
- The views share the look of the controls, not the set of the controls. The author of a view shows only the controls that its analysis needs: the agent picks the controls of each report from its conclusion (`data-controls`, and the controls of the bar), and the text says what each control shows. "Show all controls" shows the others.

Reason: 5 reviews of the views found the same controls drawn in different styles, a legend with no unit, and controls that the analysis did not use. The owner asked that each future view keeps 1 look.

The fonts are under the SIL Open Font License 1.1, each with its licence: DM Sans in `crates/viewer/fonts`, and Source Serif 4 and DM Sans in `crates/fh/src/daemon/fonts`.

---

## 9. Decision log

Each entry is a rule that is in force. The owner decided each of them.

### D-30 A requirement stays only if a real run showed the need
A requirement stays only if a real run showed the need, it gives the user a result, and you can measure it. A rule that prevents a security fault, a licence or trademark fault, or a loss of data also stays. Its reason names that fault. Reverse for one item if a real run shows the need. Then add it as a new item with an exit test.

### D-31 Rules that do not change with each new solver or case
About 10 workers add solvers and cases at the same time. These rules changed with each new solver or case, or made each pull request slow. The owner replaced them:

1. `validate_runs_every_case` checks only that no case fails. It has no count and no list of the cases. Reason: each new case changed the count, thus parallel pull requests conflicted after each merge, and main failed once when 2 pull requests both changed 14 to 15. Exit test: a pull request that adds a case does not change `crates/fh/tests/cli.rs`.
2. A new case with a written tolerance budget needs no approval. A change to the tolerance of an existing case needs the approval of the owner (`AGENTS.md`). Reason: a wider tolerance can hide a fault that an existing case found, but a new case has no earlier result to hide. Exit test: each tolerance change of an existing case on main has the approval of the owner.
3. A pull request installs only the solvers of its changed paths and runs their cases. Main runs the full suite after each merge (section 7). Reason: the full suite took about 12 minutes on each pull request, for 7 solver tools. Exit test: the full suite on main passes after each merge whose pull request passed. If it fails, the selection missed a path: correct `scripts/ci-select.sh`.
4. The table of section 6 is the only list of the solvers in the docs. CI (`scripts/ci-select.sh` and `.github/workflows/ci.yml`) and `scripts/install.sh` have the other lists of the solvers, because they must install each tool. Reason: the list was in 5 places in this file and again in `README.md`, and each new solver changed all of them. `crates/fh/src/daemon/agent.md` keeps 1 how-to section for each solver, because the chat cannot read this file. These sections are not a list of the solvers. Exit test: a new solver adds 1 row to the table and changes no other list of the solvers, except the solver lists in `scripts/ci-select.sh`, `.github/workflows/ci.yml` and `scripts/install.sh`.
5. 1 rule of discretization error for all analysis types (section 7). Reason: each analysis type had its own rule, thus a new analysis type needed a new rule. Exit test: each case states its discretization error from 2 meshes or time steps, or why it cannot.
6. The limits of section 6.1 apply to each solver process that `fh run` starts. Reason: the text named only `ccx`, but each solver already uses the same mechanism. Exit test: each solver module starts its programs only through `supervise`.
