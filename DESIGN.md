# femahoy: Product Design

Status: agreed design, before implementation.
Scope: this file describes the product only. The rules for the agents that build femahoy are in `AGENTS.md`.
Working name: "femahoy" stays provisional until the trademark, package-name, and domain checks are done (section 15).

---

## 1. Product

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

- The **engine** is the `fh` command-line program.
- The **main interface** is a visual chat, like ChatGPT or Codex. A 3D viewport and a terminal sidebar are beside the chat.
- The **chat is the leader**. It plans the work and gives tasks to teammates. The UI shows each teammate while it runs.
- The **input** is Abaqus-shaped `.inp` text.
- **Agent-facing output** is TOON.
- **Results** are VTU files, so ParaView can open them.
- The **core solver** is validated end to end against analytical references.
- Users and their agents can build their own solvers and input templates on the same crates.

Description rule: say the product is "heavily inspired by Abaqus input conventions". Never use "Abaqus" as a product name or in a product name.

---

## 2. Licence and income

- **Licence:** AGPL-3.0 for all code. A commercial licence is for sale to companies that cannot accept AGPL.
- **Contributions:** contributors sign a CLA that lets the owner relicense their code (for example, through CLA Assistant). A lawyer must check the CLA text.
- **Income:**
  1. Donations (GitHub Sponsors or similar).
  2. Commercial licences.
  3. A managed LLM tier, after the first user test. Users without their own API key pay a subscription, and femahoy supplies the model access through a hosted proxy at a margin over API cost. This tier must use API keys. A personal Claude or ChatGPT subscription cannot be resold.
- **Portfolio:** a static, read-only web demo of the viewer with stored results. It has no solver and no agents.

---

## 3. Interfaces

| Interface | Purpose |
|---|---|
| Chat | Main interface. One conversation with the leader. Every leader action shows as the `fh` command it ran. |
| Teammate tree | Shows each teammate, its state (working, blocked, done, idle), and its pane. |
| 3D viewport | Mesh with element outlines, deformed shape with a scale factor, contour plot of one field, step selector (first milestone). |
| Terminal sidebar | A real shell. Herdr runs here unchanged. |
| `fh` CLI | Works alone over SSH and tmux, with no GUI. |
| Custom panels | Buttons for frequent commands. Each button runs an `fh` command. |

Rule: the keyboard is the main input. The mouse is for the viewport and the custom panels.

---

## 4. Architecture

```
 browser (wasm UI)        native UI (eframe)        fh CLI (SSH / tmux)
        \                       |                        /
         \---- HTTP + WebSocket (127.0.0.1, token) -----/
                                |
                         fh serve (daemon)
     +-----------+-----------+-----------+-----------+-----------+
     |  solver   |   PTY     |  panes    |  agents   |  files    |
     | (in-proc  | (terminal | (herdr or | (Claude   | (project, |
     |  or sub-  |  sidebar) |  plain    |  Code,    |  results) |
     |  process) |           |  PTY)     |  Codex)   |           |
     +-----------+-----------+-----------+-----------+-----------+
```

### 4.1 Daemon (`fh serve`)

- Owns the solver runs, the PTY, the pane backend, the agent processes, the API keys, and the project files.
- Binds to 127.0.0.1 only.
- Makes a random token at start, as Jupyter does. Every HTTP and WebSocket request must carry the token.
- Rejects any request whose `Origin` is not the daemon's own page.
- Serves the wasm UI itself, then opens the user's browser at that page.
- The browser never holds an API key.

### 4.2 Clients

- **wasm UI:** the first build target. The same `eframe` code as the native UI.
- **Native UI:** the same code, built for macOS and then for Windows.
- **`fh` CLI:** talks to a running daemon, or runs the solver directly when no daemon runs.

### 4.3 Pane backend

The daemon defines a `PaneBackend` interface with two implementations:

1. **herdr**, the default when it is installed. The daemon drives it through herdr's CLI and socket.
2. **Plain PTY**, owned by the daemon. This is used when herdr is not installed, and on Windows while herdr's Windows support is beta.

The teammate tree reads from `PaneBackend`, never from herdr directly. femahoy never bundles herdr.

---

## 5. Agents in the product

### 5.1 Runtime

- Every agent is a CLI harness process: Claude Code or Codex. femahoy has no agent loop of its own.
- Two connection paths in v1:
  1. **User's CLI login:** the harness uses the user's own Claude Code or Codex login.
  2. **User's API key:** the daemon passes the key in the process environment (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`).
- The **leader** runs in headless mode with a JSON event stream (for example, `claude -p --output-format stream-json` or `codex exec --json`). The chat renders this stream. Check the exact flags against the current harness docs during M4, because they change between versions.
- **Teammates** run in panes through `PaneBackend`.
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
- The internal model is Abaqus-shaped: parts, assembly, instances, nodes, elements, sets, surfaces, materials, sections, steps, loads, boundary conditions, interactions, and constraints mirror the keyword structure.
- `fh check <file.inp>` parses and checks a model, then prints a TOON summary.

### 6.2 Meshes

- v1 imports meshes from `.inp` files made by other tools, and from gmsh `.msh` files.
- The daemon can call gmsh as an external program, so agents can mesh geometry. Users install gmsh themselves. femahoy never links gmsh, because gmsh is GPL.
- femahoy has no mesher of its own.

### 6.3 User templates

- A user can define an input syntax of their own.
- A template is a mapping file from that syntax onto the Abaqus-shaped model, with its own validation cases.
- The mapping format is an open item (section 14).

---

## 7. Solver

### 7.1 Core capabilities

All of these are required. They arrive one per release after the first user test (section 13).

| Capability | Method |
|---|---|
| Static analysis (implicit, general) | Abaqus-style static step |
| Linear algebra | `faer`, sparse Cholesky for small systems, Jacobi-preconditioned conjugate gradients (PCG) for large ones. The switch rule is in section 11.2. |
| Constraints | DOF elimination with a master-slave transformation. The matrix stays symmetric positive definite. |
| Nonlinear solution | Newton-Raphson, load stepping, automatic cutback |
| Contact | Frictionless normal contact, active-set loop, augmented Lagrange, node-to-surface |
| Plasticity | J2 with isotropic hardening, hypoelastic-plastic, corotational stress rate, updated Lagrangian |
| Large displacement | `NLGEOM=YES` through the deformation gradient F, which is passed to the material. The exact finite-strain formulation (corotational or other) is chosen at M7. |
| Element birth and death | `*MODEL CHANGE`. Removed elements leave the stiffness, and their stresses are set to zero. The mesh stays fixed. |
| Submodelling | Displacement-driven boundary. Global displacements are interpolated at the driven nodes with the global mesh's shape functions. |

### 7.2 Elements

| Element type | Use |
|---|---|
| `C3D8I` | Default hex8. Incompatible modes for good bending results. |
| `C3D8` | Hex8 with selective reduced integration (B-bar), for plasticity and near-incompressible flow. |
| `C3D10` | Default tet10. |
| `C3D10M` | Modified tet10 for contact surfaces. Corner-node forces stay correct under pressure. |

Element API: `element::evaluate` is a pure function on fixed-size arrays. It takes the node positions (`Placement`: start position plus displacement), a `Material` and the committed state of the element. It writes the internal force, the consistent tangent, the stress at each integration point and the trial state into an `Evaluation`. It does not allocate.

- The element gives each integration point its deformation gradient F. The material returns the stress, the tangent and its new state. Elements know no material constants.
- The material owns the state of each integration point (`Material::state_len` numbers, zero for elastic). The internal dofs of `C3D8I` are element state.
- `StateStore` keeps a committed copy and a trial copy of all state, in one flat array for each copy (struct-of-arrays layout).

For imported input, the `TYPE=` on `*ELEMENT` sets the element. For models that femahoy agents create, use `C3D8I` and `C3D10` unless the task needs the others.

### 7.3 Keywords

Scope: every keyword the validation cases need, in priority order. Any other keyword fails with a clear error that names the keyword and the line number.

Starting list:
- **Structure:** `*HEADING`, `*INCLUDE`, `*PART`, `*END PART`, `*ASSEMBLY`, `*INSTANCE`, `*END INSTANCE`, `*END ASSEMBLY`
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

### 7.5 Steps, increments and later analysis types

The step code (`core-solver/src/step.rs`) has one increment loop for all analysis types (D-10).

- A step is a list of increments. The load factor runs from 0 to 1. Loads and prescribed values ramp from their values at the start of the step to the values of the step.
- Each increment builds its constraint transform from the prescribed values at its load factor. A transform with the same map keeps the matrix and the factor. A transform with the same structure keeps the pattern and the solver, and builds the slot map again. Any other transform gets a new pattern.
- **Linear fallback:** when all materials are linear and the model has no contribution, a step is one increment. The solver assembles and factors once for each constraint map. No Newton code runs.
- **Newton path:** all other models. `*STATIC` gives the first, minimum and maximum increment. Each Newton iteration builds the residual and the tangent in one sweep. A converged increment commits the trial state. A failed increment drops the trial state, and the solver tries a smaller increment (cutback). An increment fails when Newton does not converge, or when a material or a contribution rejects the trial.
- **Sweep:** one call of a pure kernel for each element and each contribution group. Threads compute chunks of 256 entries. One thread adds the chunks in entry order, so the result is bit-identical for any thread count.
- A converged increment writes its result files and, when it is due, the restart file (section 9.3).

How the later milestones plug in:

| Milestone | Interface that exists in M2 | What the milestone adds | What stays |
|---|---|---|---|
| M5 Nonlinear solver | The Newton path, the cutback and the state store. Tests use a test material with state. | `C3D10M`, the nonlinear fixtures, and a test that a linear case gives the same result in both paths. | The increment loop and the restart file. |
| M6 Plasticity | `Material` with `state_len() > 0` and `is_linear() == false`. `update` gets F and the committed state. It returns the stress, the tangent and the trial state. An error cuts the increment back. | The J2 return map in a new `Material`. | The element code. Elements pass F and do not know the material law. |
| M6 Contact | `Contribution`: groups of nodes, a state for each group, and `evaluate`, which returns the force and the tangent of a group at the current positions. The groups are extra entries after the elements, so the node graph, the pattern and the slot map include them. The restart file keeps their state. | The contact search, the augmented Lagrange update and the change of the groups. A change of the groups at an increment builds the node graph and the pattern again, as a change of the constraint structure does. In M2 the groups are fixed for a run. | The step loop and the assembly. |
| M7 Large displacement | `Placement` holds the start position and the displacement. The element computes F from them. Pressure loads use the current positions. | The geometric stiffness and the finite-strain stress measure in the element kernel, and the material update for large strain. | The material interface and the Newton loop. |

---

## 8. Results and postprocessing

- **Results files:** the solver writes three formats for each converged increment. The files of the last increment of step N are `step-N.*`, and the files of the other increments are `step-N-inc-k.*`. All formats are on by default. `fh-solve --formats text,vtu,vtk` selects a subset.
  - **Plain text** (`step-N.txt`): first line `fh-results 1`, a header (units note, step, step name, time, stress order), then three tables: one row for each node (`U1 U2 U3 RF1 RF2 RF3`, and `UR1 UR2 UR3` when the model has rotations), one row for each element (`S11 S22 S33 S12 S23 S13 MISES`, volume average), and one row for each integration point (`element_id ip S11 ... MISES`). Reals use the shortest text that reads back to the same value. The model has no unit system, so the units line says that all values are in the units of the input file.
  - **VTU** (`step-N.vtu`): binary data appended raw. Sets are integer arrays. femahoy writes and reads these files in Rust, with no C library. There is no compression, because that needs a new dependency and costs time on a slow CPU.
  - **Legacy VTK** (`step-N.vtk`): ASCII `UNSTRUCTURED_GRID`, version 3.0. It has the same mesh and fields as the VTU file.
  - A PVD collection file lists the first grid format that is on (VTU, else VTK, else text). The text file is for people and scripts, and ParaView cannot read it.
  - Every writer takes one array at a time from a buffered file and makes no copy of a field. The solver result of the step stays in memory while the files are written. The integration point stress (48 bytes for each point) is the largest array and is kept only when the text format is on. Switch the text format off for models of millions of elements (about 1.5 GB of text for 1M `C3D8` elements).
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
| `Solver` trait | Trusted Rust solvers | A crate implements the trait and registers by name. The core solver is the reference implementation. |
| Subprocess protocol | Agent-written or untrusted solvers | An executable takes an `.inp` file path and a run folder, and writes result files plus a TOON summary. The protocol is versioned (version 2, section 9.3). |

Users can use the open `inp`, `io`, and `toon` crates in their own solvers.

### 9.2 Limits for subprocess solvers

- Each run has its own working folder.
- Each run has a time limit and a memory limit.
- Results from a solver that has not passed the validation suite are marked **unvalidated**.
- Later: an OS sandbox (Landlock on Linux, `sandbox-exec` on macOS) with no network access and write access to the run folder only.

The agents themselves run under the harness's own permission system.

### 9.3 Subprocess protocol, version 2

Command line: `fh-solve <input.inp> <run-dir> [--restart] [--formats text,vtu,vtk] [--restart-every N] [--restart-minutes T]`. The environment variable `FH_THREADS` sets the number of threads for the element work and the PCG solve (default: all cores). The results are bit-identical for any count.

- **stdout** is line based. The first line is `fh-solver-protocol 2`. Then these event lines:
  - `estimate bytes=N`: the memory estimate of section 11.2, before the work starts.
  - `progress step=i/n phase=<name> fraction=<0..1>`: about 20 lines for each phase.
  - `restart restart.fh`: after each write of the restart file. The solver writes it at the end of each step, after every N converged increments (`--restart-every`, default 1), and when T minutes have passed since the last write (`--restart-minutes`). The file holds the last converged increment, its state and the hash of the input.
  - `step N max_u=... max_mises=... max_rf=... free_dofs=... method=... iterations=...`: at the end of each step.
  - `stats dofs=... nnz=... pattern_builds=... estimate_bytes=... assemblies=... factorizations=... orphans=...` and `timing assemble=... factor=... solve=...` (seconds): at the end of the run.
  - The runner keeps any other line as a log line.
- **stdin**: the line `cancel`, or the end of stdin, stops the run. The exit code is 130. The runner keeps the pipe open while the solver runs.
- **Restart**: `--restart` reads `restart.fh` in the run folder. It deletes the result files after the restart point, and continues from the last converged increment. It refuses an input file that changed.
- **Run folder**: `step-N.*` for the last increment of step N, `step-N-inc-k.*` for the other increments, the PVD collection, `restart.fh`, and `summary.toon`.
- **Summary**: `summary.toon` follows the `fh.run` schema (TOON schema, sections 3.5 and 3.8). It has the header (`source`, `source_hash`, `units`, `status`), `run_id`, `solver`, `results`, `wall_s`, one row for each step (`name`, `status`, `reached`, `increments`), one row for each increment (`step`, `inc`, `iters`, `result` ok or cutback), and the diagnostics. A dof with no stiffness gives a warning with the code `SOL101`. The solver fixes that dof at zero.
- **Limits**: the runner stops the whole process group at the time limit or at the memory limit. It reads the memory use of the group with an operating system probe (Linux, macOS and Windows). The default memory limit is 85 percent of the RAM of the machine. A process that ignores `cancel` is killed after a grace period.


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
| Submodelling | Lamé cylinder sub-region, compared with the analytical field and with a fine global model |
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
- A change to the core solver merges only if the full suite passes, and a convergence check passes for each changed element or contact path.
- A failure blocks the merge. There is no override.

---

## 11. Workspace and budget

Cargo workspace. The cap is **85k lines of Rust, tests included**. Data files do not count: fixture `.inp` files, reference values, VTU files, and generated code. CI counts lines with `tokei` and fails when a crate goes over its budget.

| Crate | Scope | Licence role | Budget |
|---|---|---|---|
| `inp` | `.inp` reader and writer, Abaqus-shaped model, template mapping | open API | 9k |
| `io` | gmsh `.msh` import, VTU and PVD write and read | open API | 5k |
| `toon` | agent-facing summaries and queries | open API | 3k |
| `solver-api` | `Solver` trait, subprocess protocol, registry | open API | 3k |
| `core-solver` | elements, DOF elimination, Newton with cutback, contact, plasticity, NLGEOM, birth and death, submodelling | validated core | 30k |
| `validation` | harness, tolerance classes, manufactured solutions | validated core | 6k |
| `daemon` | HTTP and WebSocket server, token auth, PTY, pane backends, process supervision | app | 7k |
| `agents` | harness start, JSON stream parsing, skills install, role config | app | 4k |
| `cli` | `fh` commands | app | 3k |
| `viewer` | eframe app, wgpu mesh and contour renderer, terminal widget, chat panel, teammate tree | app | 15k |
| **Total** | | | **85k** |

Rules:
- A crate that goes over budget needs a written reason before it grows more.
- Review the whole budget at the first user test, with real line counts.

### 11.1 Key dependencies

| Dependency | Use | Note |
|---|---|---|
| `faer` | sparse linear algebra | pure Rust |
| `eframe` / `egui` / `wgpu` | UI and 3D renderer, native and wasm | |
| `alacritty_terminal` | terminal emulator core | Pin an exact version. Wrap it behind our own interface. |
| herdr | pane backend | External program. Never bundled. Confirm its licence. |
| gmsh | meshing | External program. Never linked (GPL). |

---

### 11.2 Target machine, memory and the solver switch

**Target machine:** a workstation of 2010. x86-64 with SSE4.2 and no AVX, 4 to 6 cores, 16 GB RAM, a hard disk or a slow SSD. Input files are small. Results files are not: write them in pieces.

Requirements:

1. A model of 1M `C3D8` elements (about 3.1M dofs) solves inside 16 GB. Measured peak memory footprint: 6.1 GiB (below).
2. The build uses the baseline x86-64 target (`-C target-cpu=nehalem` is the test setting: SSE4.2, no AVX). `faer` and its dense kernels pick AVX2 and AVX-512 code at run time with CPU feature detection, so a CPU without AVX uses the SSE code. The femahoy crates contain no SIMD code of their own. The check (PR 10): a search of the crates and the CI files finds no `std::arch`, `core::arch`, `target_feature`, `asm!` or `target-cpu`, and the repo has no `.cargo/config`. No x86-64 build with `-C target-cpu=nehalem` and no run on a CPU without AVX has been done yet. Do both on real hardware before the first user test.
3. If the matrix does not fit, the iterative solver is used. Its memory is about the size of the matrix. The out-of-core scheme is not needed so far, and is not built.
4. The default memory limit of a run is 85 percent of the RAM of the machine.

**Assembly:** the sparse pattern is built once from the node adjacency of the elements and the contribution groups (no triplets of the whole model). A slot map holds, for each element, the position in the matrix values of each entry of its lower triangle. Values are added in place through the slot map, with no search. The pattern stays across steps and Newton iterations, and is rebuilt only when the constraint structure changes (section 7.5). Element work runs in chunks of 256 entries on all threads. The main thread adds the chunks in entry order, so the matrix is bit-identical for any thread count.

**Switch rule (`Method::Auto`):** use sparse Cholesky when the reduced system has at most 60 000 free dofs, else PCG with a Jacobi preconditioner (relative residual 1e-10). Reason: Cholesky memory grows as n^1.35 (about 400 bytes times n^1.35) and its time grows faster. PCG memory is linear.

**PCG on threads:** the PCG solve runs on `FH_THREADS` threads. The rows are cut into fixed blocks of 1024 rows, and each thread owns a run of whole blocks. Each row of the matrix product has one owner and adds its terms in column order. Each dot product is a sum for each block, and the threads add the block sums in block order. Thus the result is bit-identical for any thread count. The matrix keeps only its lower triangle. For the product, the PCG solver keeps a row copy of the strict lower triangle (12 bytes for each entry).

**Memory estimate** (bytes, printed as `estimate bytes=` before the work starts), with N nodes, E elements, n dofs, nnz stored matrix entries, T threads:

    model    = 40 N + 130 E
    mesh     = 45 E + 30 (N + E) + graph + 8 N
    matrix   = 12 nnz + 4 n
    slots    = sum over elements of (4 m (m + 1) / 2 + 8), m = element dofs
    state    = 4 * (bytes of one copy of the state)
    vectors  = 24 n + 48 n + (72 n + 12 (nnz - n) + 12 n + 4 if PCG, else 24 n)
    factor   = 400 n^1.35                      (Cholesky only)
    results  = 72 N + 56 E + 120 N + 70 E + (384 E if text format)
    chunks   = min(T * 256, E) * size_of(Evaluation)
    estimate = 1.35 * (model + mesh + matrix + slots + state + vectors + factor + results + chunks)

The factor 1.35 covers allocator slack and the input text. The measured peak memory footprint of the PCG runs below is 1.03 to 1.10 times the sum.

**Measurements** (release build, macOS aarch64, Apple M5 with 10 cores and 16 GB, all 3 result formats). The peak is the peak memory footprint from `/usr/bin/time -l`. The resident set size leaves out compressed pages, so it can be lower than the footprint. The machine ran other work at the same time, so the times change from run to run (the 100k solve on 10 threads took 1.9 to 3.9 s in 3 runs). The times below come from the run with the lowest load.

| Model | Elements | Free dofs | Method | Threads | Peak | Assemble | Factor | Solve | Wall time |
|---|---|---|---|---|---|---|---|---|---|
| Cantilever and Lamé case meshes (8 meshes) | 4 to 512 | 48 to 2 176 | Cholesky | 1 or 10 | 1.7 to 12 MiB | at most 3.1 ms | at most 3.0 ms | at most 0.1 ms | 8 to 21 ms |
| `C3D8` box 100x40x25, one tip load | 100 000 | 319 800 | PCG, 656 iterations | 1 | 578 MiB | 0.58 s | 0.02 s | 8.9 s | 10.5 s |
| same | 100 000 | 319 800 | PCG, 656 iterations | 10 | 599 MiB | 0.18 s | 0.02 s | 2.0 s | 3.2 s |
| `C3D8` box 100x100x100, one tip load | 1 000 000 | 3 060 300 | PCG | 1 | 6 047 MiB | 6.2 s | 0.66 s | 129 s | 146 s |
| same | 1 000 000 | 3 060 300 | PCG | 10 | 6 065 MiB | 2.2 s | 0.38 s | 59 s | 72 s |

For PCG, "Factor" is the row copy and the preconditioner. The PCG solve is bound by memory bandwidth, so 10 threads give 4.4 times the speed at 100k elements and 2.2 times at 1M elements. The results of 1 thread and 10 threads are bit-identical (all result files of the 100k run compared).

For the 1M-element model the estimate is 7.4 GiB against a measured peak of 6.1 GiB. All files of the run (3 formats, PVD, restart file and summary) take 2.28 GB. A 2010 machine has slower cores, less memory bandwidth and a slower disk, so expect longer times. The memory numbers do not depend on the CPU.

Switch rule measurements (before the parallel PCG, generated `C3D8` box, one tip load, 10 cores, peak is the resident set size):

| Elements | Free dofs | Method | Peak | Wall time |
|---|---|---|---|---|
| 8 000 | 26 460 | Cholesky | 349 MiB | 1.0 s |
| 27 000 | 86 490 | Cholesky | 1 754 MiB | 11.5 s |
| 27 000 | 86 490 | PCG | 106 MiB | 0.7 s |

---

## 12. Platforms and distribution

1. **Web (wasm) first.** The user installs `fh`, runs `fh serve`, and the UI opens in the browser.
2. **Native macOS second.**
3. **Windows third.** Uses the plain PTY backend until herdr's Windows support is stable.

Distribution: a single `fh` binary (Homebrew and a direct download). The static demo is published on a separate web page.

---

## 13. Milestones

| Milestone | Content | Done when |
|---|---|---|
| M0 Skeleton | Workspace with all crates. `fh --help`. CI: fmt, clippy, test, `tokei` budget check. | CI is green. |
| M1 Parser and harness | `inp` reads the keyword subset for linear static. Gmsh `.msh` import. Validation harness with the first three fixtures (patch test, cantilever, Lamé). TOON summary schema v1. Fixture schema. | `fh check` works on all fixtures. `fh validate` runs and reports "no solver". |
| M2 Linear solver | `C3D8I`, `C3D8`, `C3D10`. DOF elimination. Sparse solver (`faer` Cholesky) that fits 1 million `C3D8` elements in 16 GB (D-06). Boundary conditions, loads, `*COUPLING`, `*EQUATION`, `*MPC`. Restart that is safe against partial files (D-09). Three result formats: plain text, VTU with PVD, legacy VTK (D-04). `Material` trait with elasticity, and steps that keep the later analysis types possible (D-10). Subprocess protocol v2. | All linear fixtures pass. Manufactured-solution rates pass. Restart test passes. Builds and passes on aarch64 (D-07). |
| M3 Daemon and viewer | `fh serve` with token and `Origin` checks. wasm viewer: mesh, outlines, deformed shape, one contour field, step selector. Terminal sidebar. Both pane backends. | The viewer shows the M2 results in a browser. Herdr runs in the sidebar. |
| M4 Chat and agents | Leader in headless mode in the chat. Teammates in panes. Teammate tree. `femahoy.toml`. `fh init` installs skills. Both connection paths. | The leader builds and solves a cantilever model from one chat request, and every action shows as an `fh` command. |
| **First user test** | M0 to M4, plus the static demo. Budget review. | |
| M5 Nonlinear solver | Newton with load stepping and cutback. A linear fallback for the same model. Assembly stays ready for parallel work. `C3D10M`. | Nonlinear fixtures pass. A linear case gives the same result in both paths. |
| M6 Plasticity and contact | J2 plasticity with isotropic hardening, through the `Material` trait. Contact is a separate contribution to the system, not part of the element. Augmented Lagrange node-to-surface contact. | Hill cylinder, uniaxial bar and Hertz cases pass. |
| M7 Large displacement | `NLGEOM=YES` through the deformation gradient, so that elements and materials use one path. | Elastica and rigid-rotation cases pass. |
| M8 Birth and death | `*MODEL CHANGE`. | Statically determinate cases pass. |
| M9 Submodelling | `*SUBMODEL`, `*BOUNDARY, SUBMODEL`. | Lamé sub-region passes against both references. |
| M10 Native macOS | Native build. | Same tests as M3 and M4, native. |
| M11 Windows | Windows build, plain PTY backend. | Same tests, on Windows. |

Later, with no milestone number yet: a GPU backend (Metal first, D-08), `fh mesh`, and a mesher written in Rust (D-02).

**Status:** M0 and M1 are done. M2 is in review (PR 10). **Not built yet:** everything from M3 on, the GPU backend, `fh mesh` and the Rust mesher. The decision log (section 17) states the build status of each decision.

The managed LLM tier is a separate track. It starts after the first user test.

---

## 14. Open items

| Item | Decide in |
|---|---|
| TOON summary and query schema | M1 |
| Validation fixture schema | M1 |
| Subprocess protocol schema | M2 |
| Daemon HTTP and WebSocket API | M3 |
| Template mapping format | After the first user test |
| Friction in contact | After M6 |
| OS sandbox for subprocess solvers | After M10 |
| Managed LLM tier (proxy, accounts, billing) | After the first user test |
| NAFEMS cases | After the legal check |

---

## 15. Checks before public launch

- A lawyer checks: the "heavily inspired by Abaqus" wording, use of Abaqus element and keyword names, the CLA text, and the NAFEMS use.
- Trademark, package-name (crates.io, npm, PyPI), GitHub org, and domain checks for "femahoy".
- Read herdr's `LICENSE` file. Sources disagree (Apache-2.0 or AGPL-3.0-or-later). If it is AGPL, the commercial edition must not bundle it. The current design never bundles it.
- Check the Anthropic and OpenAI terms for the managed tier.

---

## 16. Glossary

| Term | Meaning |
|---|---|
| Leader | The chat. Plans and delegates. Writes no solver code. |
| Teammate | An agent that the leader starts, with its own role and skills. |
| Validator | The teammate that runs the suite and gates merges. Separate from the leader. |
| Pane backend | The interface that runs teammates: herdr or plain PTY. |
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

### D-02 gmsh stays external, `fh mesh` is approved, own Rust mesher later
- **Decided by:** the captain.
- **Choice:** femahoy calls gmsh as an external program and never links it. The command `fh mesh` is approved. A mesher written in Rust comes later.
- **Reason:** gmsh is GPL, and the commercial licence (section 2) forbids linking GPL code. A Rust mesher removes the need for the user to install gmsh.
- **Rejected:** link gmsh, bundle gmsh.
- **Reverse if:** gmsh changes its licence, or the Rust mesher is good enough to replace it.
- **Built:** `.msh` import only. `fh mesh` and the Rust mesher are not built yet.

### D-03 TOON is for agents, VTU keeps full precision
- **Decided by:** the captain.
- **Choice:** TOON output is only for agent reading. VTU files keep the full-precision values. A `precision` key sets the significant figures in TOON. The default is 4.
- **Reason:** Fewer digits save agent tokens. Full data stays in the files for exact work.
- **Rejected:** full precision in TOON, one fixed precision with no key.
- **Reverse if:** agents need more digits to make correct decisions.
- **Built:** yes (`toon` crate, `--precision`).

### D-04 Result formats are plain text, VTU and legacy VTK
- **Decided by:** the captain.
- **Choice:** The solver writes plain text, VTU (with PVD) and legacy VTK. It does not write HDF5 or Exodus.
- **Reason:** These formats need no large library. HDF5 and Exodus need C libraries that are hard to build on all platforms.
- **Rejected:** HDF5, Exodus.
- **Reverse if:** a model is too big for these formats, or a customer needs one of the rejected formats.
- **Built:** M2 (PR 10). `fh-solve --formats` selects a subset.

### D-05 Model input stays Abaqus-style `.inp`, TOML holds only case metadata
- **Decided by:** the captain.
- **Choice:** The model is an Abaqus-style `.inp` file. TOML files hold case metadata only, such as references and tolerances. TOML never holds model data.
- **Reason:** One model format keeps the parser small and keeps Abaqus users at home.
- **Rejected:** a TOML model format, a second model format next to `.inp`.
- **Reverse if:** the template mapping (section 14) shows that a second format is needed.
- **Built:** yes (`inp` and `validation` crates).

### D-06 Target machine
- **Decided by:** the captain.
- **Choice:** The target is x86-64 with SSE4.2 and no AVX, 4 to 6 cores, 16 GB of RAM, and a slow disk. A model with 1 million C3D8 elements must solve within 16 GB.
- **Reason:** This is the machine of the expected user.
- **Rejected:** code that needs AVX, solvers that need more than 16 GB or fast disk swap.
- **Reverse if:** the captain changes the target machine.
- **Built:** M2 (PR 10): the memory estimate, the solver switch rule and the parallel PCG. The 1M-element run is measured in section 11.2. No run on a CPU without AVX has been done yet.

### D-07 Code builds and passes on aarch64
- **Decided by:** the captain.
- **Choice:** The primary test machine is an Apple Silicon MacBook. All code builds and passes tests on aarch64 with default flags.
- **Reason:** The captain tests on that machine.
- **Rejected:** x86-only intrinsics, flags that only work on x86-64.
- **Reverse if:** the captain changes the test machine.
- **Built:** a rule for all crates. The M2 gate passes on an Apple M5 (aarch64). CI does not check aarch64 yet.

### D-08 GPU later, with neutral interfaces
- **Decided by:** the captain.
- **Choice:** The solver must work on a GPU later. Interfaces stay neutral. Metal is the GPU API on the Mac. CUDA alone is not enough.
- **Reason:** The test machine has a Metal GPU.
- **Rejected:** CUDA-only design.
- **Reverse if:** no GPU backend is ever needed.
- **Built:** the interfaces only (M2): the `LinearSolver` and `Assemble` traits name no device, and the state store uses a struct-of-arrays layout. No GPU code exists.

### D-09 Restart is safe against partial files
- **Decided by:** the captain.
- **Choice:** A run can die part way. A restart must not read a partial file. Writers write to a temporary file and rename it when complete.
- **Reason:** Long runs on a slow disk can die at any time.
- **Rejected:** write in place, trust any file that exists.
- **Reverse if:** a better atomic method exists on all target platforms.
- **Built:** M2 (PR 10). Every result file and the restart file are written to a temporary name and renamed. The restart file has a version and the hash of the input. A test kills the solver at 3 random restart points and checks that the resumed files are equal to the files of a run with no kill.

### D-10 Element and step design stays open to later analysis types
- **Decided by:** the captain.
- **Choice:** The element and step design must stay compatible with parallel assembly, nonlinear analysis (with a linear fallback), plasticity, contact and large displacement. Material is separate from element properties.
- **Reason:** A change of these interfaces later costs more than a careful design now.
- **Rejected:** element code that holds material laws, a design for linear analysis only.
- **Reverse if:** a later milestone shows that the interfaces cannot carry one of these analysis types.
- **Built:** M2 (PR 10): the `Material` trait, the `Contribution` trait, the state store, and one increment loop with the linear fallback and the Newton path (section 7.5).
