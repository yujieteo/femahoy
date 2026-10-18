# femahoy: Product Design

Status: agreed design. The status of each milestone is in `docs/milestones.md`.
Scope: this file describes the product only. The rules for the agents that build femahoy are in `AGENTS.md`.
Working name: "femahoy" stays provisional until the trademark, package-name, and domain checks are done (section 15).

---

## 1. Product

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

- The **engine** is the `fh` command-line program.
- The **main interface** is a visual chat, like ChatGPT or Codex. A 3D viewport is beside the chat.
- The **chat is the leader**. It plans the work and gives tasks to teammates. The UI shows each teammate while it runs.
- The **input** is Abaqus-shaped `.inp` text.
- **Agent-facing output** is TOON.
- **Results** are VTU files, so ParaView can open them.
- The **solver** is CalculiX (`ccx`). The `fh-solve-ccx` adapter runs it (D-13). Each validation case passes against its reference.
- Users and their agents can build their own solvers and input templates on the same crates.

Description rule: say the product is "heavily inspired by Abaqus input conventions". Never use "Abaqus" as a product name or in a product name.

---

## 2. Licence and income

- **Licence:** AGPL-3.0 for all code. A commercial licence is for sale to companies that cannot accept AGPL.
- **Contributions:** contributors sign a CLA that lets the owner relicense their code (for example, through CLA Assistant). A lawyer must check the CLA text.
- **Income:**
  1. Donations (GitHub Sponsors or similar).
  2. Commercial licences.
  3. A managed LLM tier, after the second user test (section 13). Users without their own API key pay a subscription, and femahoy supplies the model access through a hosted proxy at a margin over API cost. This tier must use API keys. A personal Claude or ChatGPT subscription cannot be resold.
- **Portfolio:** a static, read-only web demo of the viewer with stored results. It has no solver and no agents.

---

## 3. Interfaces

| Interface | Purpose |
|---|---|
| Chat | Main interface, in a panel on the left of the page. One conversation with the leader. Every leader action shows as the `fh` command it ran. The text box is always available: a message during the work reaches the leader at its next step. A status shows the current action, the solve and its progress, and each failure. |
| Teammate tree | Shows each teammate, its state (working, blocked, done, idle), and its pane. |
| 3D viewport | In the center of the page. Mesh with element outlines, deformed shape with a scale factor, contour plot of one field, step selector (first milestone). |
| `fh` CLI | Works alone over SSH and tmux, with no GUI. |
| Custom panels | Buttons for frequent commands. Each button runs an `fh` command. |

Rule: the keyboard is the main input. The mouse is for the viewport and the custom panels.

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
             |  solver   |  agents   |  files    |
             | (sub-     | (Claude   | (project, |
             |  process) |  Code,    |  results) |
             |           |  Codex)   |           |
             +-----------+-----------+-----------+
```

### 4.1 Daemon (`fh serve`)

- Owns the solver runs, the agent processes, the API keys, and the project files.
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

The daemon has no pane backend. The chat replaced the terminal sidebar, so the sidebar and its plain PTY backend are removed. herdr is removed (D-27). The teammate tree (section 3) will define how the daemon runs teammates.

---

## 5. Agents in the product

### 5.1 Runtime

- Every agent is a CLI harness process: Claude Code or Codex. femahoy has no agent loop of its own.
- Two connection paths in v1:
  1. **User's CLI login:** the harness uses the user's own Claude Code or Codex login.
  2. **User's API key:** the daemon passes the key in the process environment (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`).
- The **leader** runs in headless mode with a JSON event stream (for example, `claude -p --output-format stream-json` or `codex exec --json`). The chat renders this stream. Check the exact flags against the current harness docs during M4, because they change between versions.
- The leader process stays alive between the messages, and each message goes to its stdin (`claude -p --input-format stream-json`). Thus a message during the work reaches the leader at its next step (`docs/chat.md`).
- **Teammates** are not built. Section 4.3 has the status.
- The leader starts teammates with `fh team spawn`, so each start is a visible command.

### 5.2 Roles

| Role | Job | Default model |
|---|---|---|
| Leader | Plans, delegates, merges accepted work. Writes no solver code. | Strongest model the user has |
| Modeler | Builds and checks `.inp` models (preprocessing). | Mid tier |
| Solver developer | Writes and changes user solver code. | Mid tier |
| Postprocessor | Results queries, TOON summaries, plots. | Mid tier |
| Validator | Runs the validation suite, reviews changes. | Strongest model the user has |

Rules:
- `femahoy.toml` in each project sets the harness and model for each role.
- The validator always runs as a separate process from the leader.
- When both vendors are available, the validator uses the other vendor. Codex checks Claude's work, and Claude checks Codex's work.
- A failed validation blocks the merge. The leader cannot override it. The leader can re-plan, re-assign, or make the task smaller.
- Each teammate works in its own git worktree and returns a structured report.

### 5.3 Skills

- Source: `.femahoy/skills/<role>/SKILL.md` in each project.
- `fh init` creates the harness wiring: entries in `.claude/skills/` for Claude Code, and an `AGENTS.md` that points Codex to the same files.
- Skills are versioned with the `fh` binary. `fh skills update` refreshes them.
- Each `SKILL.md` must have these sections: **purpose**, **inputs and outputs**, **invariants the code must keep**, **forbidden actions**, **validation hooks it must pass**, **examples**. Other content is free-form.

---

## 6. Preprocessing

### 6.1 Input

- `.inp` is the native input format.
- The internal model is Abaqus-shaped and flat: nodes, elements, sets, surfaces, materials, sections, steps, loads, boundary conditions, interactions, and constraints mirror the keyword structure.
- `fh check <file.inp>` parses and checks a model, then prints a TOON summary.

### 6.2 Meshes

- v1 imports meshes from `.inp` files made by other tools, and from gmsh `.msh` files.
- The daemon can call gmsh as an external program, so agents can mesh geometry. Users install gmsh themselves. femahoy never links gmsh, because gmsh is GPL.
- gmsh is the mesher now. A mesher of our own is a main advantage for later. It has no milestone yet (D-15).

### 6.3 User templates

- A user can define an input syntax of their own.
- A template is a mapping file from that syntax onto the Abaqus-shaped model, with its own validation cases.
- The mapping format is an open item (section 14).

---

## 7. Solver

### 7.1 Solver now

The solver is CalculiX (`ccx`). The `fh-solve-ccx` adapter runs it as a separate program (D-12, D-13). The user installs `ccx` (`docs/ccx.md`). Each solve and `fh validate` stop with an error that tells how to install it when `ccx` is not there.

The owner removed the native solver `fh-solve` on 2026-10-19 (D-29). An own solver is a main advantage later, but not now. Main commit `08a97c2` has its code and its design: the linear static solver, the Newton nonlinear path, restart, the sparse Cholesky and the Jacobi PCG solver.

Not now: element birth and death (`*MODEL CHANGE`, M8) and submodelling (M9).

### 7.2 Elements

| Element type | Use |
|---|---|
| `C3D8I` | Default hex8. Incompatible modes for good bending results. |
| `C3D8` | Hex8 with selective reduced integration (B-bar), for plasticity and near-incompressible flow. |
| `C3D10` | Default tet10. |
| `C3D10M` | Modified tet10 for contact surfaces. Corner-node forces stay correct under pressure. |

For imported input, the `TYPE=` on `*ELEMENT` sets the element. For models that femahoy agents create, use `C3D10` (D-19). Use `C3D8I` only for extruded parts, and the others only when the task needs them.

### 7.3 Keywords

Scope: every keyword the validation cases need, in priority order. Any other keyword fails with a clear error that names the keyword and the line number.

Starting list:
- **Structure:** `*HEADING`, `*INCLUDE`. The model is flat. `*PART`, `*ASSEMBLY` and `*INSTANCE` fail with a clear error.
- **Mesh and sets:** `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET`, `*SURFACE`
- **Material and section:** `*MATERIAL`, `*ELASTIC`, `*PLASTIC`, `*DENSITY`, `*SOLID SECTION`
- **Steps and loads:** `*STEP` (with `NLGEOM`), `*STATIC`, `*BOUNDARY`, `*CLOAD`, `*DLOAD`, `*DSLOAD`, `*END STEP`
- **Constraints:** `*COUPLING` with `*KINEMATIC` (RBE2 behaviour), `*COUPLING` with `*DISTRIBUTING` (RBE3 behaviour), `*EQUATION` (general linear MPC), `*MPC` (a subset of types)
- **Contact:** `*SURFACE INTERACTION`, `*SURFACE BEHAVIOR` (with `AUGMENTED LAGRANGE`), `*CONTACT PAIR`
- **Special:** `*MODEL CHANGE`, `*SUBMODEL`, `*BOUNDARY, SUBMODEL`
- **Output:** `*OUTPUT`, `*NODE OUTPUT`, `*ELEMENT OUTPUT`, `*NODE PRINT`, `*EL PRINT`

### 7.4 Compatibility

- Keyword names and meanings match Abaqus for the supported subset.
- `docs/compatibility.md` lists every difference. A test covers each difference.
- Tests use our own `.inp` files. No Abaqus manual files go in the repo (section 10.1).

---

## 8. Results and postprocessing

- **Results files:** the solver writes one VTU file for each step (`step-N.vtu`) and a PVD collection. `fh-solve-ccx` writes VTU only. The `fh-io` crate can also write plain text and legacy VTK, for solvers that need them.
  - **Plain text** (`step-N.txt`): first line `fh-results 1`, a header (units note, step, step name, time, stress order), then three tables: one row for each node (`U1 U2 U3 RF1 RF2 RF3`, and `UR1 UR2 UR3` when the model has rotations), one row for each element (`S11 S22 S33 S12 S23 S13 MISES`, volume average), and one row for each integration point (`element_id ip S11 ... MISES`). Reals use the shortest text that reads back to the same value. The model has no unit system, so the units line says that all values are in the units of the input file.
  - **VTU** (`step-N.vtu`): binary data appended raw. Sets are integer arrays. femahoy writes and reads these files in Rust, with no C library. There is no compression, because that needs a new dependency and costs time on a slow CPU.
  - **Legacy VTK** (`step-N.vtk`): ASCII `UNSTRUCTURED_GRID`, version 3.0. It has the same mesh and fields as the VTU file.
  - The PVD collection file lists the grid files. The text file is for people and scripts, and ParaView cannot read it.
  - Every writer takes one array at a time from a buffered file and makes no copy of a field.
- ParaView opens the PVD file directly.
- The wasm viewer and the submodel interpolation read the same files.
- **TOON** is used only for summaries and queries that agents ask for (for example, "maximum Mises stress in set X, step 2"). Full field data never goes into TOON.
- **Commands:** `fh results` prints TOON summaries. `fh plot` writes images. The viewer shows the same data in 3D.
- Weak point: very large models (millions of elements). A binary format of our own can come later if this becomes a problem.

---

## 9. Extensibility

### 9.1 Solver interfaces

| Interface | For | How it works |
|---|---|---|
| Subprocess protocol | Every solver: `fh-solve-ccx`, and agent-written or untrusted solvers | An executable takes an `.inp` file path and a run folder, and writes result files plus a TOON summary. The protocol is versioned (version 2, section 9.3). |

Users can use the open `inp`, `io`, and `toon` crates in their own solvers.

### 9.2 Limits for subprocess solvers

- Each run has its own working folder.
- Each run has a memory limit. A time limit applies only when the case or an option sets one.
- Results from a solver that has not passed the validation suite are marked **unvalidated**.
- Later: an OS sandbox (Landlock on Linux, `sandbox-exec` on macOS) with no network access and write access to the run folder only.

The agents themselves run under the harness's own permission system.

### 9.3 Subprocess protocol, version 2

Command line: `<solver> <input.inp> <run-dir> [--formats LIST]`. `fh-solve-ccx` accepts only `--formats vtu`. The protocol text is in `docs/solver-protocol.md`.

- **stdout** is line based. The first line is `fh-solver-protocol 2`. Then these event lines:
  - `estimate bytes=N`: the memory estimate of the solver, before the work starts.
  - `progress step=i/n phase=<name> fraction=<0..1>`: about 20 lines for each phase.
  - The runner keeps any other line as a log line.
- **stdin**: the line `cancel` stops the run. The end of stdin also stops it when `FH_CANCEL_ON_EOF` is `1`, which the runner sets. The exit code is 130. The runner keeps the pipe open while the solver runs.
- **Run folder**: `step-N.vtu` for the last increment of step N, the PVD collection and `summary.toon`. `fh-solve-ccx` also keeps the `ccx` files in `ccx/`.
- **Summary**: `summary.toon` follows the `fh.run` schema (TOON schema, sections 3.5 and 3.8). It has the header (`source`, `source_hash`, `units`, `status`), `run_id`, `solver`, `results`, `wall_s`, one row for each step (`name`, `status`, `reached`, `increments`), one row for each increment (`step`, `inc`, `iters`, `result` ok or cutback), and the diagnostics.
- **Limits**: the runner stops the whole process group at the memory limit, and at the time limit when the case or an option sets one. It reads the memory use of the group with an operating system probe (Linux, macOS and Windows). The default memory limit is 85 percent of the RAM of the machine. A process that ignores `cancel` is killed after a grace period.


---

## 10. Validation

### 10.1 References

- Analytical solutions, manufactured solutions, and fine-model comparisons.
- No Abaqus licence is available, so there are no Abaqus runs.
- The Abaqus verification manual can be used as a list of problem ideas, cited by number. Never copy its text, `.inp` files, figures, or numbers into the repo.
- NAFEMS benchmarks: add them only after a legal check. If you add them, use our own input files and cite the target values with the source.

### 10.2 Reference set

| Capability | Cases |
|---|---|
| Linear, `C3D8I`, `C3D8`, `C3D10` | Patch test, cantilever (beam theory), Lamé thick cylinder, plate with a hole (Kirsch) |
| Contact | Hertz: sphere on flat, cylinder on flat |
| Plasticity | Elastic-plastic thick cylinder (Hill), uniaxial bar |
| Large displacement | Cantilever large deflection (elastica), rigid rotation (zero stress) |
| Birth and death | Statically determinate structures where removing a member gives a reaction change you can calculate by hand |
| All elements | Method of manufactured solutions, to check the convergence rate |

### 10.3 Tolerance classes

| Class | Applies to | Rule |
|---|---|---|
| L | Linear cases | Richardson extrapolation over at least three mesh refinements. Tolerance = estimated discretisation error + the reference's own stated error. |
| N | Nonlinear and contact cases | A fixed tolerance per case, written and justified in the fixture file, checked on a named reference mesh. |
| R | Manufactured solutions | The fixture states the expected convergence rate and the allowed band. |

Contact cases compare integrated values: total contact force, contact area or radius, and peak pressure. They never compare node-by-node contact status.

### 10.4 Fixtures

- Each case is a folder in `validation/cases/<id>/` with the input files, the reference values, the tolerance class, and the justification.
- Fixtures are data files, not prose.
- The exact fixture schema is decided in M1.

### 10.5 Gate

- `fh validate` runs the suite and reports pass or fail per case.
- A change to a solver or its adapter merges only if the full suite passes, and a convergence check passes for each changed element or contact path.
- A failure blocks the merge. There is no override.

---

## 11. Workspace

Cargo workspace. Keep the code small. Delete code that is not needed, and prefer a deletion to an addition.

| Crate | Scope | Licence role |
|---|---|---|
| `inp` | `.inp` reader and writer, Abaqus-shaped model, template mapping | open API |
| `io` | gmsh `.msh` import, VTU and PVD write and read | open API |
| `toon` | agent-facing summaries and queries | open API |
| `solver-api` | subprocess protocol v2 and its runner | open API |
| `validation` | harness, tolerance classes, manufactured solutions | validated core |
| `daemon` | HTTP server, token auth, process supervision, chat agent, the page (HTML chat) | app |
| `agents` | harness start, JSON stream parsing, skills install, role config | app |
| `cli` | `fh` commands | app |
| `viewer` | eframe app, wgpu mesh and contour renderer, teammate tree | app |
| `solver-ccx` | CalculiX adapter: `ccx` input writer, process run, `.frd` reader, results as VTU, PVD and TOON (D-13) | app |

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

- The default memory limit of a run is 85 percent of the RAM of the machine. The runner stops a solver at this limit (section 9.3).
- The native solver solved a model of 1M `C3D8` elements in 5.9 GiB. Section 11.2 of `DESIGN.md` at main commit `08a97c2` has the measurements. The workflow benchmark records (B1) will measure `ccx` on this machine.

---

## 12. Platforms and distribution

1. **Web (wasm) first.** The user installs `fh`, runs `fh serve`, and the UI opens in the browser.
2. **Windows second.**

Distribution: a single `fh` binary (Homebrew and a direct download). The static demo is published on a separate web page.

---

## 13. Milestones

`docs/milestones.md` is the one roadmap. It holds each item, what it needs, its exit test and its status.

Order of the work (D-27):

1. **Now: the basic LLM workflow (M4).** The user asks the chat for a model. The leader builds the CAD, meshes it, solves it and shows the results. The user sees each step in the page of `fh serve`. One focused agent is the default. The user can add more agents later. Every action shows as an `fh` command.
2. **Now: CalculiX for thermal strain, several materials and warpage (C3).**
3. **Now: the workflow benchmark records (B1).** They measure the workflow from the request to the results.
4. **Later:** an own solver (D-29), the other CalculiX capabilities (C2), the Windows platform (M11) and the second user test.

Rules:

- An item starts when the items in its **Needs** column are done. Items that do not need each other can run at the same time.
- A milestone with stages is done only when all its stages pass.
- The captain can change the order at any time.

Later, with no milestone yet: a mesher of our own (D-15). The managed LLM tier starts after the second user test.

---

## 14. Open items

| Item | Decide in |
|---|---|
| TOON summary and query schema | M1 |
| Validation fixture schema | M1 |
| Subprocess protocol schema | M2 |
| Daemon HTTP API | M3 |
| Template mapping format | After the second user test |
| Friction in contact | After M6 |
| OS sandbox for subprocess solvers | After M4 |
| Managed LLM tier (proxy, accounts, billing) | After the second user test |
| NAFEMS cases | After the legal check |

---

## 15. Checks before public launch

- A lawyer checks: the "heavily inspired by Abaqus" wording, use of Abaqus element and keyword names, the CLA text, and the NAFEMS use.
- Trademark, package-name (crates.io, npm, PyPI), GitHub org, and domain checks for "femahoy".
- Check the Anthropic and OpenAI terms for the managed tier.

---

## 16. Glossary

| Term | Meaning |
|---|---|
| Leader | The chat. Plans and delegates. Writes no solver code. |
| Teammate | An agent that the leader starts, with its own role and skills. |
| Validator | The teammate that runs the suite and gates merges. Separate from the leader. |
| Fixture | One validation case: inputs, reference values, tolerance class, justification. |
| Template | A user input syntax with a mapping onto the Abaqus-shaped model. |
| Unvalidated | A result from a solver that has not passed the validation suite. |

---

## 17. Decision log

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
- **Reason:** gmsh is GPL, and the commercial licence (section 2) forbids linking GPL code.
- **Rejected:** link gmsh, bundle gmsh.
- **Reverse if:** gmsh changes its licence.
- **Built:** `.msh` import and `fh mesh` (`cli` crate). `fh mesh` runs gmsh as a subprocess. v1 refuses a `.geo` file that can run other programs or files.

### D-03 TOON is for agents, VTU keeps full precision
- **Decided by:** the captain.
- **Choice:** TOON output is only for agent reading. VTU files keep the full-precision values. A `precision` key sets the significant figures in TOON. The default is 4.
- **Reason:** Fewer digits save agent tokens. Full data stays in the files for exact work.
- **Rejected:** full precision in TOON, one fixed precision with no key.
- **Reverse if:** agents need more digits to make correct decisions.
- **Built:** yes (`toon` crate, `--precision`).

### D-04 Result formats are plain text, VTU and legacy VTK
- **Decided by:** the captain.
- **Choice:** The solver supports plain text, VTU (with PVD) and legacy VTK. It does not write HDF5 or Exodus. From 2026-10-18 only VTU with PVD is on by default. Text and legacy VTK are options (`--formats`).
- **Reason:** These formats need no large library. HDF5 and Exodus need C libraries that are hard to build on all platforms.
- **Rejected:** HDF5, Exodus.
- **Reverse if:** a model is too big for these formats, or a customer needs one of the rejected formats.
- **Update:** the captain decided on 2026-10-18 to change the default. A run of 1M `C3D8` elements wrote 2.28 GB with all 3 formats, and the text file was about 1.5 GB of that. The text writer also kept about 384 bytes for each element in memory. The 3 formats stay (decided 2026-10-13, for compatibility).
- **Built:** M2 (PR 10). `fh-solve --formats` selected a subset. The default was VTU with PVD (2026-10-18).
- **Update:** D-29 removed `fh-solve` on 2026-10-19. `fh-solve-ccx` writes VTU with PVD only. The `fh-io` writers for plain text and legacy VTK stay.

### D-05 Model input stays Abaqus-style `.inp`, TOML holds only case metadata
- **Decided by:** the captain.
- **Choice:** The model is an Abaqus-style `.inp` file. TOML files hold case metadata only, such as references and tolerances. TOML never holds model data.
- **Reason:** One model format keeps the parser small and keeps Abaqus users at home.
- **Rejected:** a TOML model format, a second model format next to `.inp`.
- **Reverse if:** the template mapping (section 14) shows that a second format is needed.
- **Built:** yes (`inp` and `validation` crates).

### D-06 Target machine
- **Decided by:** the captain.
- **Choice:** The target is 4 to 6 cores, 16 GB of RAM, and a slow disk. A model with 1 million C3D8 elements must solve within 16 GB.
- **Reason:** This is the machine of the expected user.
- **Rejected:** solvers that need more than 16 GB or fast disk swap.
- **Reverse if:** the captain changes the target machine.
- **Built:** M2 (PR 10) built the memory estimate, the solver switch rule and the parallel PCG in the native solver. D-29 removed them on 2026-10-19. The runner keeps the default memory limit (section 11.2). The workflow benchmark records (B1) will measure `ccx` on the target machine.

### D-07 Code builds and passes on aarch64
- **Decided by:** the captain.
- **Choice:** The primary test machine is an Apple Silicon MacBook. All code builds and passes tests on aarch64 with default flags.
- **Reason:** The captain tests on that machine.
- **Rejected:** x86-only intrinsics, flags that only work on x86-64.
- **Reverse if:** the captain changes the test machine.
- **Built:** a rule for all crates. The M2 gate passes on an Apple M5 (aarch64). CI builds and tests each change on aarch64 macOS (P1, docs/milestones.md).

### D-08 GPU later, with neutral interfaces
- **Removed:** by the captain, 2026-10-18. The GPU backend is not on the roadmap.
- **Decided by:** the captain.
- **Choice:** The solver must work on a GPU later. Interfaces stay neutral. Metal is the GPU API on the Mac. CUDA alone is not enough.
- **Reason:** The test machine has a Metal GPU.
- **Rejected:** CUDA-only design.
- **Reverse if:** no GPU backend is ever needed.
- **Built:** no GPU code exists. D-29 removed the M2 interfaces with the native solver.

### D-09 Restart is safe against partial files
- **Decided by:** the captain.
- **Choice:** A run can die part way. A restart must not read a partial file. Writers write to a temporary file and rename it when complete.
- **Reason:** Long runs on a slow disk can die at any time.
- **Rejected:** write in place, trust any file that exists.
- **Reverse if:** a better atomic method exists on all target platforms.
- **Built:** M2 (PR 10). Every result file and the restart file are written to a temporary name and renamed. The restart file has a version and the hash of the input. A test kills the solver at 3 random restart points and checks that the resumed files are equal to the files of a run with no kill.
- **Update:** D-29 removed restart with the native solver on 2026-10-19. `fh-solve-ccx` writes each result file to a temporary name and renames it.

### D-10 Element and step design stays open to later analysis types
- **Decided by:** the captain.
- **Choice:** The element and step design must stay compatible with parallel assembly, nonlinear analysis (with a linear fallback), plasticity, contact and large displacement. Material is separate from element properties.
- **Reason:** A change of these interfaces later costs more than a careful design now.
- **Rejected:** element code that holds material laws, a design for linear analysis only.
- **Reverse if:** a later milestone shows that the interfaces cannot carry one of these analysis types.
- **Built:** M2 (PR 10): the `Material` trait, the `Contribution` trait, the state store, and one increment loop with the linear fallback and the Newton path.
- **Update:** D-29 removed this code on 2026-10-19. Main commit `08a97c2` has it. An own solver later must keep this decision.

### D-11 Results are the same bits for any thread count
- **Decided by:** the captain.
- **Choice:** Parallel work must scale. The parallel solver gives the same bits for any thread count. Assembly uses a reduction in a fixed order.
- **Reason:** Results must not depend on the machine. Byte-equal results make crash-resume and tests exact.
- **Rejected:** results that change with the thread count.
- **Reverse if:** a measured conflict with D-06 makes the captain choose speed over exact equality.
- **Built:** M2 (PR 10). `crates/core-solver/tests/threads.rs` compared the result files and the restart files of 1, 2, 4 and all threads.
- **Update:** D-29 removed the native solver and this test on 2026-10-19. An own solver later must keep this decision.

### D-12 Own solver, and other solvers as separate programs
- **Decided by:** the captain. Changed by D-27 and D-29.
- **Choice:** An own solver is a main advantage of the product, later (D-29). Other open solvers run as separate programs that the user installs. femahoy adds an adapter crate for each one. "One package" means one femahoy package with the adapters. It never contains the solver programs.
- **Reason:** External solvers give capabilities now. Separate programs keep GPL code out of the product (D-02).
- **Rejected:** link or bundle another solver.
- **Reverse if:** the captain changes the focus of the product.
- **Built:** yes. `solver-ccx` is the first adapter crate (D-13).

### D-13 CalculiX is the first external solver
- **Decided by:** the captain.
- **Choice:** CalculiX (`ccx`) is the first external solver. It runs as a separate program from a new crate, `solver-ccx`. The user installs `ccx`. Contact in `ccx` uses the penalty method. A rule sets the penalty value, and a sweep in the validation case tests it. Our own Rust reader reads the `ccx` `.frd` result files.
- **Reason:** Its input is closest to our `.inp`. It passed all 6 capability tests (linear, plasticity, contact, large movement, model change, submodel). It is GPL, so it stays a separate program. `ccx` supports only penalty contact, and a bad penalty value gave a 10 percent error.
- **Rejected:** link or bundle `ccx`, contact with no rule for the penalty value.
- **Reverse if:** `ccx` cannot pass the C1 stage, or its licence or input format changes.
- **Built:** in part. Stage C1 built `solver-ccx` for linear static (`docs/ccx.md`). Contact and the penalty rule come in stage C2.

### D-14 `ccx` serves M6 and M7 first
- **Decided by:** the captain.
- **Choice:** The built-in solver gets the M5 Newton work. `ccx` gives the M6 and M7 capabilities first (stage C2) and cross-checks the built-in results. The built-in M6 and M7 milestones stay on the roadmap.
- **Reason:** Nonlinear features arrive early, and `ccx` cross-checks the built-in results.
- **Rejected:** M6 and M7 in the built-in solver before users get these capabilities.
- **Reverse if:** `ccx` fails the C2 case of a capability. Then the built-in milestone for that capability comes first.
- **Built:** not yet. Stage C1 does not wait for the workflow lock (D-21).
- **Changed:** D-29 removed the built-in solver on 2026-10-19. `ccx` gives the M5, M6 and M7 capabilities (stage C2). No built-in results exist to cross-check.

### D-15 gmsh now, a mesher of our own later
- **Decided by:** the captain. Changed by D-27.
- **Choice:** gmsh is the mesher now. Users install gmsh and other tools. These tools run as separate programs and are never bundled or linked. A mesher of our own is a main advantage of the product for later. It has no milestone yet.
- **Reason:** The basic LLM workflow comes first, and gmsh already works in it. gmsh is GPL, and the commercial licence rules out GPL code in the product (D-02).
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
- **Built:** the first path, for a bar (M2b, `docs/cad-recipe.md`): a build123d script, a `.geo` template for gmsh, and `fh info` for the checks. `fh model` turns the `.msh` file and the `[model]` table of the case into a solver input (case `bar-tension`). Named regions are built: `fh mesh` makes a gmsh physical surface for each `[[model.region]]` of the case, and `fh model` writes it as a node set and an element-face surface (cases `bar-pressure` and `bar-pressure-m`). `fh mesh` is built (D-18, D-19).

### D-18 The case declares units, `fh` refuses a mismatch
- **Decided by:** the captain.
- **Choice:** The case TOML declares the units. `fh` refuses to run when the units do not match, when the case has no unit, and when a STEP file has no unit record.
- **Reason:** A wrong unit gives wrong results without any warning.
- **Rejected:** a default unit, a guessed unit, a warning in place of the refusal.
- **Reverse if:** the captain allows a run with unknown units.
- **Built:** in part (M2b). The case TOML has a required `units` key (`SI-mm` or `SI-m`). `fh info` refuses a STEP file with no unit record (`unit.step.missing`), with different unit records (`unit.step.conflict`) or with another unit than the case (`unit.step.mismatch`). `fh mesh` takes the units from `--case DIR` or `--units`, has no default, and refuses a conflict (`unit.case.conflict`), no units (`unit.case.missing`) and the STEP unit faults of `fh info`. `fh mesh` writes a unit record beside the mesh (`OUT.msh.units`). `fh model` refuses a mesh with no record (`unit.mesh.missing`), with another unit than the case (`unit.mesh.mismatch`) or with a record that conflicts with `--mesh-units` (`unit.mesh.conflict`). `fh model` writes the line `** units: SI-mm` (or `SI-m`) into the input, and `fh-solve-ccx` writes those units into `summary.toon` and the PVD file.

### D-19 C3D10 is the default element
- **Decided by:** the captain.
- **Choice:** `C3D10` (tet10) is the default element. Hex8 (`C3D8I`, `C3D8`) is only for extruded parts.
- **Reason:** Tetrahedra mesh any shape. Quadratic elements do not have the bending errors of plain hex8. In the `cantilever` validation case (length 10 mm, 1 x 1 mm section, nu 0.3) with 1 element through the section and the element type changed to `C3D8`, plain hex8 (`C3D8`, B-bar) gave 0.35 times the beam deflection with 4 elements along the beam, and 2.07 times with 32 elements. The mesh rule is in `docs/compatibility.md`.
- **Rejected:** hex8 as the default element.
- **Reverse if:** the workflow benchmark records (B1) show that typical tet10 models do not fit D-06.
- **Built:** the M2b recipe makes `C3D10` meshes from CAD, and `ccx` solves them. The native `C3D10` element (M2, PR 10) left with D-29. `fh info` refuses a mesh with an inverted element (`mesh.jacobian.negative`). `fh info` and `fh mesh` refuse a hex8 mesh that is not one extrusion (`crates/cli/src/rules.rs`).

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
- **Choice:** Remove the no-AVX x86-64 target (SSE4.2, no AVX, 2010 workstation) and its gates: item E3, `scripts/check-no-avx.sh`, a run on a real CPU with no AVX, the repeat of the 1M `C3D8` run on x86-64 under an emulator, and the "no swap" proof for the 1M run. The 16 GB target of D-06 and its evidence stay. The thread-count test and the same-bits rule (D-11) stay.
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
  1. M4 starts now. The two tracks and the workflow lock are removed (this replaces D-16). `scripts/check-workflow.sh` stays the proof of the CLI workflow.
  2. The M4 exit test is the basic LLM workflow: one chat request gives a solved cantilever, and the page of `fh serve` shows each step. M4 needs one connection path, the user's CLI login. The API key path comes after the M4 exit test.
  3. New item C3: thermal strain, several materials and warpage through CalculiX, now.
  4. New item B1 replaces E2: benchmark records measure the workflow, from the request to the results.
  5. The native nonlinear solver stays on the roadmap, later. N1 and N2 are no longer separate items before M5 and M6. Their checks are in the exit tests of M5, M6 and M7.
  6. gmsh is the mesher now. A mesher of our own is a main advantage for later, with no milestone yet (D-15). The native solver is a main advantage (D-12).
  7. `docs/milestones.md` is the one roadmap. Section 13 holds the order and the rules only. The second user test does not need the static demo.
  8. A crate `SKILL.md` does not repeat a rule of `AGENTS.md`. `README.md` does not repeat the decision log.
  9. herdr is removed. The terminal sidebar uses the plain PTY backend. A separate PR removes the herdr code.
  10. M4 uses 1 focused agent by default, with the option to add more agents later.
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
- **Reverse if:** the teammate tree or the custom panels need widgets that the page cannot give.

### D-29 Native solver removed
- **Decided by:** the owner, 2026-10-19. The owner said: "It is a main advantage but not now, besides it is recorded in git?"
- **Choice:** Remove the native solver `fh-solve` (`crates/core-solver`, about 11 600 lines): the linear static solver, the Newton nonlinear path, restart, the sparse Cholesky and the Jacobi PCG solver. `fh-solve-ccx` is the default solver of `fh run`, `fh validate`, `fh measure`, the daemon and `scripts/install.sh`. The subprocess protocol v2 and its runner stay.
- **Reason:** An own solver is a main advantage later, but not now. Git keeps the code: main commit `08a97c2` has the full native solver. CalculiX through `fh-solve-ccx` passes the validation suite.
- **Rejected:** keep the native solver beside `fh-solve-ccx`.
- **Reverse if:** the owner gives an own solver a milestone. Start from commit `08a97c2`.
- **Built:** yes. This changes D-12, D-14 and item 5 of D-27. Restart (D-09) and the native solver work of D-04, D-06, D-08, D-10 and D-11 are removed.
