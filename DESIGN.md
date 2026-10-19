# femahoy: Product Design

Status: agreed design. The status of each milestone is in `docs/milestones.md`.
Scope: this file describes the product only. The rules for the agents that build femahoy are in `AGENTS.md`.

---

## 1. Product

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

- The **engine** is the `fh` command-line program.
- The **main interface** is a visual chat, like ChatGPT or Codex. A 3D viewport is beside the chat.
- The **chat is the leader**: 1 focused agent that plans and does the work.
- The **input** is Abaqus-shaped `.inp` text.
- **Agent-facing output** is plain text lines (D-31).
- **Results** are VTU files, so ParaView can open them.
- The **solver** is CalculiX (`ccx`). `fh` starts it directly (D-13, D-31). Each validation case passes against its reference.
- Users and their agents can build their own solvers on the same crates.

Description rule: say the product is "heavily inspired by Abaqus input conventions". Never use "Abaqus" as a product name or in a product name.

---

## 2. Licence

- **Licence:** AGPL-3.0 for all code.
- CalculiX and gmsh stay separate programs. femahoy never links or bundles them, so that a later sale stays possible.

---

## 3. Interfaces

| Interface | Purpose |
|---|---|
| Chat | Main interface, in a panel on the left of the page. One conversation with the leader. Every leader action shows as the `fh` command it ran. The text box is always available: a message during the work reaches the leader at its next step. A status shows the current action, the solve and its progress, and each failure. |
| 3D viewport | In the center of the page. Mesh with element outlines, deformed shape with a scale factor, contour plot of one field, step selector (first milestone). |
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
             | (sub-     | (Claude   | (project, |
             |  process) |  Code)    |  results) |
             +-----------+-----------+-----------+
```

### 4.1 Daemon (`fh serve`)

- Owns the solver runs, the agent process, and the project files.
- Binds to 127.0.0.1 only.
- Makes a random token at start, as Jupyter does. Every HTTP request must carry the token.
- Rejects any request whose `Origin` is not the daemon's own page.
- Serves the page and the wasm viewer itself, then opens the user's browser at that page.
- The browser never holds an API key.

### 4.2 Clients

- **Page:** the only UI. HTML and CSS with a small plain script, served by the daemon, with no build tool and no file from the network. It holds the chat and the list of the runs, in the Inter font (D-28).
- **wasm viewer:** the 3D viewport in a canvas of the page, built with `eframe` (egui with wgpu). egui draws only the viewport and its controls. The native UI is removed (D-20).
- **`fh` CLI:** talks to a running daemon, or runs the solver program directly when no daemon runs.

### 4.3 Pane backend

The daemon has no pane backend. The chat replaced the terminal sidebar, so the sidebar and its plain PTY backend are removed. herdr is removed (D-27).

---

## 5. Agent in the product

### 5.1 Runtime

- The agent is a CLI harness process: Claude Code, with the user's own Claude Code login. femahoy has no agent loop of its own.
- The **leader** runs in headless mode with a JSON event stream (`claude -p --output-format stream-json`). The chat renders this stream. Check the exact flags against the current harness docs, because they change between versions.
- The leader process stays alive between the messages, and each message goes to its stdin (`claude -p --input-format stream-json`). Thus a message during the work reaches the leader at its next step (`docs/chat.md`).

---

## 6. Preprocessing

### 6.1 Input

- `.inp` is the native input format.
- The internal model is Abaqus-shaped and flat: nodes, elements, sets, surfaces, materials, sections, steps, loads, boundary conditions, interactions, and constraints mirror the keyword structure.

### 6.2 Meshes

- v1 imports meshes from `.inp` files made by other tools, and from gmsh `.msh` files.
- The daemon can call gmsh as an external program, so agents can mesh geometry. Users install gmsh themselves. femahoy never links gmsh, because gmsh is GPL.
- gmsh is the mesher now. A mesher of our own is a main advantage for later. It has no milestone yet (D-15).

---

## 7. Solver

### 7.1 Solver now

The solver is CalculiX (`ccx`). `fh` starts it directly as a separate program (D-12, D-13, D-31). The user installs `ccx` (`docs/ccx.md`). Each solve and `fh validate` stop with an error that tells how to install it when `ccx` is not there.

The owner removed the native solver `fh-solve` on 2026-10-19 (D-29). An own solver is a main advantage later, but not now. Main commit `08a97c2` has its code and its design: the linear static solver, the Newton nonlinear path, restart, the sparse Cholesky and the Jacobi PCG solver.

Not now: element birth and death (`*MODEL CHANGE`, M8) and submodelling (M9).

### 7.2 Elements

| Element type | Use |
|---|---|
| `C3D10` | Tet10, the only element (D-19). |

The parser refuses every other `TYPE=` on `*ELEMENT` with a clear error. A gmsh `.msh` file must hold type 11 (`C3D10`) volumes.

### 7.3 Keywords

Scope: the keywords that the validation cases need. Any other keyword fails with a clear error that names the keyword and the line number. `docs/reference/inp.md` gives the parameters and the limits.

- **Structure:** The model is flat. `*PART`, `*ASSEMBLY` and `*INSTANCE` fail with a clear error.
- **Mesh and sets:** `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET`, `*SURFACE, TYPE=ELEMENT`
- **Material and section:** `*MATERIAL`, `*ELASTIC`, `*EXPANSION`, `*SOLID SECTION`, `*INITIAL CONDITIONS, TYPE=TEMPERATURE`
- **Steps and loads:** `*STEP`, `*STATIC`, `*BOUNDARY`, `*CLOAD`, `*DLOAD`, `*DSLOAD`, `*TEMPERATURE`, `*END STEP`

### 7.4 Compatibility

- Keyword names and meanings match Abaqus for the supported subset.
- The subset has no constraints (`*COUPLING`, `*EQUATION`, `*MPC`), no `NLGEOM` and no element other than `C3D10`. Each of them fails with a clear error.
- `docs/compatibility.md` lists every difference. A test covers each difference.
- Tests use our own `.inp` files. No Abaqus manual files go in the repo (section 10.1).

---

## 8. Results and postprocessing

- **Results files:** the solver writes one VTU file for each step (`step-N.vtu`) and a PVD collection.
  - **VTU** (`step-N.vtu`): binary data appended raw. Sets are integer arrays. femahoy writes and reads these files in Rust, with no C library. There is no compression, because that needs a new dependency and costs time on a slow CPU.
  - The PVD collection file lists the grid files.
  - Every writer takes one array at a time from a buffered file and makes no copy of a field.
- ParaView opens the PVD file directly.
- The wasm viewer reads the same files.
- Agents read plain text lines. Full field data stays in the VTU files.
- Weak point: very large models (millions of elements). A binary format of our own can come later if this becomes a problem.

---

## 9. Extensibility

### 9.1 Solver interface

`fh` starts `ccx` directly, in the process of `fh serve` or of the `fh` command (D-31). The `solver-ccx` crate writes the `ccx` input, runs `ccx` and writes the results. There is no solver protocol, because only 1 solver exists. Another solver can get its own adapter crate when a real run needs it.

Users can use the open `inp` and `io` crates in their own solvers.

### 9.2 Limits of the `ccx` process

- Each run has its own working folder.
- Each run has a memory limit. The default is 85 percent of the RAM of the machine. A time limit applies only when the case or an option sets one.
- A cancel, the memory limit and the time limit kill the whole process group of `ccx` at once. A guard process kills the group when `fh` stops.
- The run reads the memory use of the group with an operating system probe (Linux and macOS).
- The run folder has `step-N.vtu` for the last increment of step N, the PVD collection, and the `ccx` files in `ccx/`. There is no summary file. The run record has the state and the time.
- The chat and the page show the state of the run and the time.

The agent itself runs under the permission system of its harness. `docs/ccx.md` has the details.

---

## 10. Validation

### 10.1 References

- Analytical solutions, measured values and fine-model comparisons.
- No Abaqus licence is available, so there are no Abaqus runs.
- The Abaqus verification manual can be used as a list of problem ideas, cited by number. Never copy its text, `.inp` files, figures, or numbers into the repo.
- NAFEMS benchmarks: add them only after a legal check. If you add them, use our own input files and cite the target values with the source.

### 10.2 Reference set

| Capability | Cases |
|---|---|
| Linear, `C3D10` | Bar in tension and in pressure, cantilever (beam theory), Lamé thick cylinder, plate with a hole (Kirsch), plate on its edge (fine model) |
| Thermal strain, `C3D10` | Bilayer strip (Lubarda), measured warpage of a wafer slice |
| Contact | Hertz: sphere on flat, cylinder on flat |
| Plasticity | Elastic-plastic thick cylinder (Hill), uniaxial bar |
| Large displacement | Cantilever large deflection (elastica), rigid rotation (zero stress) |
| Birth and death | Statically determinate structures where removing a member gives a reaction change you can calculate by hand |

### 10.3 Tolerances

Each case has 1 mesh and a fixed tolerance, written and justified in the fixture file.
A quantity passes if `|computed - reference| <= rel_tol * |reference| + abs_tol`.
The tolerance holds the error of that mesh and the stated error of the reference.

Contact cases compare integrated values: total contact force, contact area or radius, and peak pressure. They never compare node-by-node contact status.

### 10.4 Fixtures

- Each case is a folder in `validation/cases/<id>/` with its mesh file, the reference values, the tolerances and the justification.
- Fixtures are data files, not prose.
- The fixture schema is in `crates/validation/SCHEMA.md` (schema 3).

### 10.5 Gate

- `fh validate` runs the suite and reports pass or fail per case.
- A change to a solver or its adapter merges only if the full suite passes.
- A failure blocks the merge. There is no override.

---

## 11. Workspace

Cargo workspace. Keep the code small. Delete code that is not needed, and prefer a deletion to an addition.

| Crate | Scope | Licence role |
|---|---|---|
| `inp` | `.inp` reader, Abaqus-shaped model | open API |
| `io` | gmsh `.msh` import, VTU and PVD write and read | open API |
| `validation` | harness and the pass rule | validated core |
| `daemon` | HTTP server, token auth, process supervision, chat agent, the page (HTML chat) | app |
| `cli` | `fh` commands | app |
| `viewer` | eframe app, wgpu mesh and contour renderer | app |
| `solver-ccx` | CalculiX adapter: `ccx` input writer, process run, `.frd` reader, results as VTU and PVD (D-13, D-31) | app |

### 11.1 Key dependencies

| Dependency | Use | Note |
|---|---|---|
| CalculiX (`ccx`) | solver | External program. Never linked (GPL). |
| `eframe` / `egui` / `wgpu` | 3D viewport and its controls, wasm | |
| Inter | font of the page and the viewer | SIL Open Font License 1.1. In `crates/viewer/fonts`, with its licence. |
| gmsh | meshing | External program. Never linked (GPL). |

---

### 11.2 Target machine and memory

**Target machine:** 16 GB RAM, 4 to 6 cores, a hard disk or a slow SSD (D-06). Input files are small. Results files are not: write them in pieces.

- The default memory limit of a run is 85 percent of the RAM of the machine. The run stops `ccx` at this limit (section 9.2).
- The native solver solved a model of 1M `C3D8` elements in 5.9 GiB. Section 11.2 of `DESIGN.md` at main commit `08a97c2` has the measurements.

---

## 12. Platforms and distribution

The platforms are macOS and Linux. The user installs `fh` with `scripts/install.sh`, runs `fh serve`, and the UI opens in the browser.

---

## 13. Milestones

`docs/milestones.md` is the one roadmap. It holds each item, what it needs, its exit test and its status.

Order of the work (D-27):

1. **Now: the basic LLM workflow (M4).** The user asks the chat for a model. The leader builds the CAD, meshes it, solves it and shows the results. The user sees each step in the page of `fh serve`. One focused agent does the work. Every action shows as an `fh` command.
2. **Now: CalculiX for thermal strain, several materials and warpage (C3).**

Rules:

- An item starts when the items in its **Needs** column are done. Items that do not need each other can run at the same time.
- A milestone with stages is done only when all its stages pass.
- The captain can change the order at any time.

Later, with no milestone yet: a mesher of our own (D-15) and an own solver (D-29).

---

## 14. Glossary

| Term | Meaning |
|---|---|
| Leader | The agent of the chat. Plans and does the work. Writes no solver code. |
| Fixture | One validation case: mesh, reference values, tolerances, justification. |

---

## 15. Decision log

Each entry has an ID, the choice, the reason, the rejected options, and what would reverse it. All entries below are decided by the captain. Code that shows a decision has a comment `// Decision D-xx, see DESIGN.md`. A decision with no such comment is not built yet.

### D-01 Fixture schema is TOML
- **Decided by:** the captain.
- **Choice:** Each validation case has a `case.toml` file.
- **Reason:** TOML is easy for people and agents to read and edit. It has comments.
- **Rejected:** JSON (no comments), YAML (many parse traps).
- **Reverse if:** the harness needs a structure that TOML cannot express.
- **Built:** yes (`validation` crate).

### D-02 gmsh stays external, `fh mesh` is approved
- **Decided by:** the captain.
- **Choice:** femahoy calls gmsh as an external program and never links it. The command `fh mesh` is approved. A mesher of our own comes later (D-15).
- **Reason:** gmsh is GPL. Section 2 keeps it a separate program.
- **Rejected:** link gmsh, bundle gmsh.
- **Reverse if:** gmsh changes its licence.
- **Built:** `.msh` import and `fh mesh` (`cli` crate). `fh mesh` runs gmsh as a subprocess. v1 refuses a `.geo` file that can run other programs or files.

### D-03 TOON is for agents, VTU keeps full precision
- **Decided by:** the captain. Changed by D-31.
- **Choice:** TOON output is only for agent reading. VTU files keep the full-precision values. A `precision` key sets the significant figures in TOON. The default is 4.
- **Reason:** Fewer digits save agent tokens. Full data stays in the files for exact work.
- **Rejected:** full precision in TOON, one fixed precision with no key.
- **Reverse if:** agents need more digits to make correct decisions.
- **Built:** removed by D-31 on 2026-10-19. The `toon` crate and the TOON summary are deleted.

### D-04 Results are VTU with PVD
- **Decided by:** the captain.
- **Choice:** Results are VTU with PVD. femahoy does not write HDF5 or Exodus.
- **Reason:** VTU needs no large library, and ParaView opens it. HDF5 and Exodus need C libraries that are hard to build.
- **Rejected:** HDF5, Exodus.
- **Reverse if:** a real model is too big for VTU.
- **Built:** yes. The `ccx` run writes VTU with PVD. D-30 removed the plain text and legacy VTK writers on 2026-10-19, because no solver wrote these files after D-29.

### D-05 Model input stays Abaqus-style `.inp`, TOML holds only case metadata
- **Decided by:** the captain.
- **Choice:** The model is an Abaqus-style `.inp` file. TOML files hold case metadata only, such as references and tolerances. TOML never holds model data.
- **Reason:** One model format keeps the parser small and keeps Abaqus users at home.
- **Rejected:** a TOML model format, a second model format next to `.inp`.
- **Reverse if:** a real run shows that a second format is needed.
- **Built:** yes (`inp` and `validation` crates).

### D-06 Target machine
- **Decided by:** the captain.
- **Choice:** The target is 4 to 6 cores, 16 GB of RAM, and a slow disk. A model with 1 million C3D8 elements must solve within 16 GB.
- **Reason:** This is the machine of the expected user.
- **Rejected:** solvers that need more than 16 GB or fast disk swap.
- **Reverse if:** the captain changes the target machine.
- **Built:** M2 (PR 10) built the memory estimate, the solver switch rule and the parallel PCG in the native solver. D-29 removed them on 2026-10-19. The `ccx` run keeps the default memory limit (section 11.2).

### D-07 Code builds and passes on aarch64
- **Decided by:** the captain.
- **Choice:** The primary test machine is an Apple Silicon MacBook. All code builds and passes tests on aarch64 with default flags.
- **Reason:** The captain tests on that machine.
- **Rejected:** x86-only intrinsics, flags that only work on x86-64.
- **Reverse if:** the captain changes the test machine.
- **Built:** a rule for all crates. The M2 gate passes on an Apple M5 (aarch64). CI builds and tests each change on aarch64 macOS (P1, docs/milestones.md).

### D-09 No reader sees a partial file
- **Decided by:** the captain.
- **Choice:** A run can die part way. A reader must never see a partial file. Writers write to a temporary file and rename it when complete.
- **Reason:** Long runs on a slow disk can die at any time.
- **Rejected:** write in place, trust any file that exists.
- **Reverse if:** a better atomic method exists on all target platforms.
- **Built:** the `ccx` run writes each result file to a temporary name and renames it. D-29 removed restart with the native solver on 2026-10-19.

### D-12 Own solver, and other solvers as separate programs
- **Decided by:** the captain. Changed by D-27, D-29 and D-31.
- **Choice:** An own solver is a main advantage of the product, later (D-29). Other open solvers run as separate programs that the user installs. femahoy adds an adapter crate for each one. "One package" means one femahoy package with the adapters. It never contains the solver programs.
- **Reason:** External solvers give capabilities now. Separate programs keep GPL code out of the product (D-02).
- **Rejected:** link or bundle another solver.
- **Reverse if:** the captain changes the focus of the product.
- **Built:** yes. `solver-ccx` is the first adapter crate (D-13). Since D-31, `fh` calls the adapter crate in its own process and starts `ccx` directly.

### D-13 CalculiX is the first external solver
- **Decided by:** the captain.
- **Choice:** CalculiX (`ccx`) is the first external solver. It runs as a separate program from a new crate, `solver-ccx`. The user installs `ccx`. Our own Rust reader reads the `ccx` `.frd` result files.
- **Reason:** Its input is closest to our `.inp`. It passed all 6 capability tests (linear, plasticity, contact, large movement, model change, submodel). It is GPL, so it stays a separate program.
- **Rejected:** link or bundle `ccx`.
- **Reverse if:** `ccx` cannot pass the C1 stage, or its licence or input format changes.
- **Built:** yes. Stage C1 built `solver-ccx` for linear static (`docs/ccx.md`). C3 added thermal strain and several materials. D-31 removed the `fh-solve-ccx` program between `fh` and `ccx`.

### D-15 gmsh now, a mesher of our own later
- **Decided by:** the captain. Changed by D-27.
- **Choice:** gmsh is the mesher now. Users install gmsh and other tools. These tools run as separate programs and are never bundled or linked. A mesher of our own is a main advantage of the product for later. It has no milestone yet.
- **Reason:** The basic LLM workflow comes first, and gmsh already works in it. gmsh is GPL, so it stays a separate program (section 2).
- **Rejected:** link or bundle gmsh.
- **Reverse if:** the captain gives the mesher of our own a milestone.
- **Built:** no mesher code exists. The mesh path is the `.msh` import (D-02).

### D-16 Two tracks: the validated workflow first
- **Decided by:** the captain.
- **Choice:** The roadmap has two tracks (section 13). Track 1 is one complete validated workflow with the CLI: text-to-CAD, units check, gmsh, built-in solver, validated results. Track 2 holds the later milestones: the viewer, the chat, the external solvers and the nonlinear work. Track 2 starts after the workflow lock. Exceptions: D1, M3 and C1. The lock needs U1 and the passing `scripts/check-workflow.sh` proof only (D-26).
- **Reason:** One trusted base comes before more features are built on it.
- **Rejected:** the viewer, the chat or the nonlinear work before the workflow is validated, and the first user test only after M4.
- **Reverse if:** the first user test shows that the workflow needs a track 2 item before the workflow lock.
- **Replaced by:** D-27. The two tracks and the workflow lock are removed.
- **Built:** M0, M1 and the M2 solver are on main. `docs/milestones.md` has the status of each track 1 item.

### D-17 Text-to-CAD through Python, STEP through gmsh
- **Decided by:** the captain.
- **Choice:** Text-to-CAD uses a Python script, with build123d recommended. The script writes STEP. STEP import goes through gmsh with OpenCASCADE. These tools run as separate programs that the user installs.
- **Reason:** Python CAD libraries exist, and a chat can write the script. gmsh already includes OpenCASCADE, so STEP goes through gmsh.
- **Rejected:** a CAD kernel or a STEP reader inside `fh`.
- **Reverse if:** build123d or gmsh cannot write or read STEP on a supported platform.
- **Built:** the first path, for a bar (M2b, `docs/cad-recipe.md`): a build123d script, and `fh mesh` for gmsh and the checks. `fh model` turns the `.msh` file and the `[model]` table of the case into a solver input (case `bar-tension`). Named regions are built: `fh mesh` makes a gmsh physical surface for each `[[model.region]]` of the case, and `fh model` writes it as a node set and an element-face surface (cases `bar-pressure` and `bar-pressure-m`). `fh mesh` is built (D-18, D-19).

### D-18 The case declares units, `fh` refuses a mismatch
- **Decided by:** the captain.
- **Choice:** The case TOML declares the units. `fh` refuses to run when the units do not match, when the case has no unit, and when a STEP file has no unit record.
- **Reason:** A wrong unit gives wrong results without any warning.
- **Rejected:** a default unit, a guessed unit, a warning in place of the refusal.
- **Reverse if:** the captain allows a run with unknown units.
- **Built:** in part (M2b). The case TOML has a required `units` key (`SI-mm` or `SI-m`). `fh mesh` takes the units from `--case DIR` or `--units`, has no default, and refuses a conflict (`unit.case.conflict`) and no units (`unit.case.missing`). It refuses a STEP file with no unit record (`unit.step.missing`), with different unit records (`unit.step.conflict`) or with another unit than the case (`unit.step.mismatch`). `fh mesh` writes a unit record beside the mesh (`OUT.msh.units`). `fh model` refuses a mesh with no record (`unit.mesh.missing`), with another unit than the case (`unit.mesh.mismatch`) or with a record that conflicts with `--mesh-units` (`unit.mesh.conflict`). `fh model` writes the line `** units: SI-mm` (or `SI-m`) into the input, and `fh-solve-ccx` writes those units into `summary.toon` and the PVD file.

### D-19 C3D10 is the only element
- **Decided by:** the captain.
- **Choice:** `C3D10` (tet10) is the only element (2026-10-19). Before that, hex8 (`C3D8I`, `C3D8`) was allowed for extruded parts.
- **Reason:** Tetrahedra mesh any shape. Quadratic elements do not have the bending errors of plain hex8. In the old `cantilever` case (1 x 1 mm section, 1 element through it), plain hex8 (`C3D8`, B-bar) gave 0.35 to 2.07 times the beam deflection. `docs/compatibility.md` at main commit `50dded8` has the data. 1 element type keeps the parser, the adapter, the viewer and the cases small.
- **Rejected:** hex8 as the default element, and hex8 for extruded parts.
- **Reverse if:** the workflow benchmark records (B1) show that typical tet10 models do not fit D-06.
- **Built:** the M2b recipe makes `C3D10` meshes from CAD, and `ccx` solves them. The native `C3D10` element (M2, PR 10) left with D-29. `fh mesh` refuses a mesh with an inverted element (`mesh.jacobian.negative`). The parser, the `.msh` reader and the viewer accept only `C3D10`.

### D-20 Early mesh-only viewer
- **Decided by:** the captain.
- **Choice:** A mesh-only viewer (`fh view MODEL.inp`, native, no results) is built before the workflow lock. This is an exception to D-16. The viewer shows the surface mesh, the element edges and the outline of the part with orbit, pan and zoom.
- **Reason:** The interface to the harness workflow must ship soon. The viewer needs no daemon and does not change the validated solver path.
- **Rejected:** results, deformed shape, contours, the daemon, chat and the wasm build in this step. These stay in track 2.
- **Reverse if:** the viewer work delays a track 1 item.
- **Built:** `crates/viewer` and `fh view` (`docs/viewer.md`). The M3 wasm viewer grew from this code.
- **Reversed:** the captain, 2026-10-18. `fh view`, the native window and M10 (native macOS) are removed. The wasm viewer on the page of `fh serve` is the only viewer. Thus the `fh` binary does not link eframe and wgpu.

### D-21 C1 does not wait for the workflow lock
- **Decided by:** the captain, 2026-10-18.
- **Choice:** C1 (`solver-ccx`) starts before the workflow lock. This is an exception to D-16.
- **Reason:** `solver-ccx` is a new, separate crate. It cannot change the validated solver path.
- **Built:** not yet.

### D-22 No gates for the 2010 machine
- **Decided by:** the captain, 2026-10-18.
- **Choice:** Remove the no-AVX x86-64 target (SSE4.2, no AVX, 2010 workstation) and its gates: item E3, `scripts/check-no-avx.sh`, a run on a real CPU with no AVX, the repeat of the 1M `C3D8` run on x86-64 under an emulator, and the "no swap" proof for the 1M run. The 16 GB target of D-06 and its evidence stay.
- **Reason:** The gates cost much and add little.
- **Built:** the text of D-06, M2.6, section 11.2 and the workflow lock changed. No build flag or config existed only for this target.

### D-23 No line budget
- **Decided by:** the captain, 2026-10-18.
- **Choice:** Remove the line budget: the 85k cap, the budget of each crate, `scripts/check-budget.sh` and its CI step. Delete code that is not needed. Prefer a deletion to an addition.
- **Reason:** A line count does not measure quality.
- **Built:** the script, the CI step and every budget text are removed.

### D-24 Fewer rules for build agents
- **Decided by:** the captain, 2026-10-18.
- **Choice:** `AGENTS.md` has no model table, no "one crate per task" rule and no "stop after M0" rule. The leader chooses the model. One task is one roadmap item. Milestone status is only in `docs/milestones.md`.
- **Reason:** The rules were stale.
- **Built:** `AGENTS.md` and section 13 changed.

### D-25 Birth and death, submodelling, own mesher and own heavy solver
- **Decided by:** the captain, 2026-10-18.
- **Choice:** Birth and death (M8) and submodelling (M9) are on a "not now" list (section 7.1). D-27 replaced the rules of this record for a mesher of our own and a heavy built-in solver.
- **Reason:** These items are not required now.
- **Built:** the text of section 7.1, D-12, D-15, `README.md` and `crates/io/SKILL.md` changed.

### D-26 Remove dumb requirements
- **Decided by:** the captain, 2026-10-18. The captain said: "Remove dumb requirements."
- **Choice:** Remove these 5 requirements:
  1. E2 no longer requires "no swap". A shared Mac cannot prove it. D-22 removed the same proof from the 1M run.
  2. E2 is not in track 1. The benchmark records against CalculiX measure its workloads after the workflow lock.
  3. The lock needs U1 and the passing `scripts/check-workflow.sh` proof only. E2, the open part of P1 and the hex8 rule in the run command are not lock gates. The hex8 rule (D-19) stays. The CI gap of P1 (CI has no gmsh) is a small separate task.
  4. U1 uses the browser viewer. ParaView is optional.
  5. M8 and M9 are not in the track 2 tables or in the scope of C2. The "not now" list (section 7.1, D-25) holds them.
- **Reason:** These requirements blocked the focus work and a shared Mac cannot prove them.
- **Built:** the text of section 13, D-14, D-16 and `docs/milestones.md` changed.

### D-27 The basic LLM workflow first
- **Decided by:** the captain, 2026-10-18. The captain said: "biggest problem is getting the basic workflow to work?", "make requirements less dumb", "Delete herdr, I just want to talk" and "1 focused agent focusing on the right thing with the option to add more agents is OK".
- **Choice:**
  1. M4 starts now. The two tracks and the workflow lock are removed (this replaces D-16). `fh validate` is the proof of the CLI workflow (the script `scripts/check-workflow.sh` was deleted).
  2. The M4 exit test is the basic LLM workflow: one chat request gives a solved cantilever, and the page of `fh serve` shows each step. M4 needs one connection path, the user's CLI login.
  3. New item C3: thermal strain, several materials and warpage through CalculiX, now.
  4. New item B1 replaces E2: benchmark records measure the workflow, from the request to the results. D-30 removed B1.
  5. The native nonlinear solver stays on the roadmap, later. D-29 removed the native solver, and D-30 removed M5, M6 and M7.
  6. gmsh is the mesher now. A mesher of our own is a main advantage for later, with no milestone yet (D-15). The native solver is a main advantage (D-12).
  7. `docs/milestones.md` is the one roadmap. Section 13 holds the order and the rules only.
  8. A crate `SKILL.md` does not repeat a rule of `AGENTS.md`. `README.md` does not repeat the decision log.
  9. herdr is removed. The terminal sidebar uses the plain PTY backend. A separate PR removes the herdr code.
  10. M4 uses 1 focused agent.
  11. The GPU backend is removed from the roadmap (D-08).
- **Reason:** The captain will ask an LLM to do the workflow steps (U1). Thus the workflow from the chat to the results in the page of `fh serve` is the most important problem. Requirements that do not serve it, C3 or B1 wait.
- **Rejected:** the workflow lock as a gate for M4. An own mesher or the native nonlinear solver before M4.
- **Reverse if:** the captain changes the order in section 13.

### D-28 An HTML page, with the chat at the side and the model in the center
- **Decided by:** the captain, 2026-10-19. The captain said: "It should look like ChatGPT somehow", "Use Inter", "Only the renderer requires the egui right?", "Chat should not be in the center. It should be at the side, with the FEM in the center." and "do not disable the textbook I should be able to interrupt the worker just like now".
- **Choice:**
  1. The page of `fh serve` is HTML and CSS with a small plain script, and no build tool or package manager. The chat is a panel on the left. The 3D viewport fills the center.
  2. egui draws only the 3D viewport and its controls. The chat and the list of the runs are HTML.
  3. The font is Inter, in the viewer. The page loads no file from the network.
  4. The text box of the chat is always available. A message during the work reaches the leader at its next step. `Stop` ends the work at once.
  5. While the leader works, the chat shows the current action, the solve and its progress, the time, and each failure.
- **Reason:** A chat needs normal text: scrolling, selection and copy. egui draws its text on a canvas. The 3D viewport needs the GPU, and egui hosts it well.
- **Rejected:** a script framework or a package manager for the page. A new renderer.
- **Reverse if:** a real run needs a widget that the page cannot give.

### D-29 Native solver removed
- **Decided by:** the owner, 2026-10-19. The owner said: "It is a main advantage but not now, besides it is recorded in git?"
- **Choice:** Remove the native solver `fh-solve` (`crates/core-solver`, about 11 600 lines): the linear static solver, the Newton nonlinear path, restart, the sparse Cholesky and the Jacobi PCG solver. `fh-solve-ccx` is the default solver of `fh run`, `fh validate`, `fh measure`, the daemon and `scripts/install.sh`. The subprocess protocol v2 and its runner stay.
- **Reason:** An own solver is a main advantage later, but not now. Git keeps the code: main commit `08a97c2` has the full native solver. CalculiX through `fh-solve-ccx` passes the validation suite.
- **Rejected:** keep the native solver beside `fh-solve-ccx`.
- **Reverse if:** the owner gives an own solver a milestone. Start from commit `08a97c2`.
- **Built:** yes. This changes D-12 and item 5 of D-27. Restart (D-09) and the native solver work of D-04 and D-06 are removed.

### D-30 Remove the plans that no real run asked for
- **Decided by:** the owner, 2026-10-19. The owner said: "yeah these requirements are stupid delete them".
- **Choice:** A requirement stays only if a real run showed the need, it gives the user a result, and you can measure it. The real runs are the chat cantilever, the owner's plate, the 2 warpage papers and the first user test. Remove these plans and their text:
  1. The team of 5 agent roles, the reviewer agent from another vendor, the teammate tree, `fh team spawn` and `femahoy.toml`. The chat uses 1 focused agent.
  2. The skills system: `fh init`, `fh skills update` and the required skill sections.
  3. The API key path and Codex. The chat uses the user's Claude Code login.
  4. User templates (own input formats) and custom panels.
  5. The rules for an own solver later: D-08, D-10, D-11, D-14, M5, M6 and M7. Main commit `08a97c2` keeps them with the code of the native solver.
  6. C2 (CalculiX plasticity, contact and large displacement). Add each capability when a paper or a real problem needs it.
  7. `fh results`, `fh plot`, the submodel results and the sandbox for solvers that agents write.
  8. Windows (M11) and Homebrew. The platforms are macOS and Linux.
  9. The income plans: donations, the commercial licence sale, the managed LLM tier, the web demo, the CLA, the lawyer and trademark checks and the rights review (R1). Section 2 keeps 1 rule: CalculiX and gmsh stay separate programs.
  10. The second user test and the workflow benchmark records (B1).
  11. The plain text and legacy VTK result writers (`crates/io/src/vtk.rs`) and the `--formats` option of `fh-solve-ccx`. No solver writes these files.
  12. The Windows code of the runner (`crates/solver-api/src/sys.rs`).
- **Reason:** The owner's goal is a FEM solver for arbitrarily large models, without paid software, that the owner can use with an LLM. The owner does not sell femahoy now. Each worker reads the design and the roadmap and builds toward them, so a plan with no need costs work.
- **Rejected:** keep the plans as "later" items.
- **Reverse if:** a real run shows the need for one of these items. Then add it as a new item with an exit test.
- **Built:** yes. This changes D-02, D-04, D-05, D-09, D-13, D-15, D-22, D-27 and D-28.

### D-31 `fh` starts `ccx` directly, and the run summary goes
- **Decided by:** the owner, 2026-10-19. The owner said: "yes do 1 2 and 3, replace toon" and then "Agree with all."
- **Choice:** `fh` starts `ccx` directly. The adapter code is a library that `fh serve` and the `fh` command call in their own process. Remove the `fh-solve-ccx` program, the subprocess protocol v2 and its runner (`crates/solver-api`), `FH_SOLVE` and the case key `solver`. Remove TOON (`crates/toon`) and the run summary. No new summary file replaces it.
- **Reason:** No run needed another solver. The summary had no result numbers, and the run record already has the state and the time. Nobody measured the token saving of TOON.
- **Rejected:** keep the protocol for a later solver. Replace TOON with a `key=value` summary.
- **Reverse if:** a real run needs a second solver, or an agent needs a summary file.
- **Built:** yes. The memory limit, the time limit, the stop of the process group and the run state in the chat stay. This changes D-03, D-12, D-13 and D-29.
