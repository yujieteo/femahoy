# femahoy

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

The engine is the `fh` command-line program. The main interface is a visual chat. A 3D viewport and a terminal sidebar are beside the chat.

The chat plans the work and gives tasks to teammates. The user interface shows each teammate while it runs.

The input is `.inp` text, heavily inspired by Abaqus input conventions. Agent-facing output is TOON. Results are VTU files, so ParaView can open them.

## Status

Agreed design. M0, M1 and the M2 linear solver are on main. The roadmap and the status of each item are in `DESIGN.md` section 13 and [docs/milestones.md](docs/milestones.md). The name is provisional. The name depends on trademark, package-name and domain checks.

## Licence

The code is licensed under AGPL-3.0.

A commercial licence is for sale to companies that cannot accept AGPL.

## Planned crates

Budgets are lines of Rust, tests included. The total cap is 85k lines.

| Crate | Scope | Budget |
|---|---|---|
| `inp` | `.inp` reader and writer, model, template mapping | 9k |
| `io` | gmsh `.msh` import, VTU and PVD write and read | 5k |
| `toon` | agent-facing summaries and queries | 3k |
| `solver-api` | `Solver` trait, subprocess protocol, registry | 3k |
| `core-solver` | elements, DOF elimination, Newton with cutback, contact, plasticity, NLGEOM, birth and death, submodelling | 30k |
| `validation` | harness, tolerance classes, manufactured solutions | 6k |
| `daemon` | HTTP and WebSocket server, token auth, PTY, pane backends, process supervision | 7k |
| `agents` | harness start, JSON stream parsing, skills install, role config | 4k |
| `cli` | `fh` commands | 3k |
| `viewer` | eframe app, wgpu mesh and contour renderer, terminal widget, chat panel, teammate tree | 12k, provisional |
| `solver-ccx` | CalculiX adapter: `ccx` input, process run, `.frd` reader, results (D-13) | 3k, provisional |
| **Total** | | **85k** |

The `viewer` and `solver-ccx` budgets are provisional until the budget review at the first user test (`DESIGN.md` section 11).

## Planned platforms

1. Web (wasm) first. The user installs `fh`, runs `fh serve`, and the user interface opens in the browser.
2. Native macOS second.
3. Windows third. It uses the plain PTY backend until herdr Windows support is stable.

The product is distributed as one `fh` binary, through Homebrew and as a direct download.

## Design decisions

This section lists the design decisions for femahoy. `DESIGN.md` section 17 holds the decision log, D-01 to D-19, with rejected options and reversal rules. The roadmap is in `DESIGN.md` section 13, and the milestone status is in [docs/milestones.md](docs/milestones.md).

### Product scope

- femahoy is an interface for real-time preprocessing, solving and postprocessing. It is dual licensed, open and commercial. Reason: the commercial licence is for companies that cannot accept AGPL-3.0.
- femahoy has no own mesher for now. Users install gmsh and other tools. External programs run as separate subprocesses and are never bundled or linked. Reason: gmsh is GPL, and the commercial licence rules out GPL code in the product (D-02).
- The built-in solver stays light on resources. Other open solvers may be integrated later, in one package. Reason: heavy kernels already exist as open tools, so the product focuses on the interface.
- The first goal is one complete validated workflow: text-to-CAD, units check, gmsh, built-in solver, validated results. It comes before the viewer, chat and nonlinear work. Reason: one trusted base comes before more features are built on it.

### Input, output and formats

- Model input stays Abaqus-style `.inp`. TOML holds only case metadata and the fixture schema, read with `toml_edit`. Reason: one model format keeps the parser small (D-05).
- Result formats are plain text, VTU with PVD (full precision) and legacy VTK. There is no HDF5 and no Exodus. Reason: these formats need no large C library (D-04).
- TOON is the summary format for agents to read. An empty table prints `name[0]:`. The `precision` key sets significant figures and defaults to 4. Reason: fewer digits save agent tokens, and VTU files keep full precision (D-03).

### Hardware and parallelism

- The primary test machine is an Apple Silicon MacBook (aarch64). All code builds and passes tests on it with default flags. Reason: the project tests on that machine (D-07).
- The target also covers x86-64 with SSE4.2 and no AVX, 4 to 6 cores, 16 GB RAM and a slow disk. A model of 1 million C3D8 elements must solve within 16 GB. Reason: this is the machine of the expected user (D-06).
- Parallel work must scale. The parallel solver gives the same bits for any thread count. Reason: results must not depend on the machine, and byte-equal results make crash-resume and tests exact.
- A run can stop at any time. A restart must not read a partial file. Reason: long runs on a slow disk can stop at any time (D-09).
- A GPU backend comes later. Metal comes first on the Mac. Interfaces stay GPU-neutral, not CUDA-only. Reason: the test machine has a Metal GPU (D-08).

### Solver design

- Elements and steps follow a fixed interface. The deformation gradient F goes to the `Material` trait. State sits at integration points, with committed and trial copies. Reason: a later change of these interfaces costs more than a careful design now (D-10).
- The solver uses a Newton loop with cutback and a linear fallback, parallel assembly with fixed-order reduction, and contact as a separate contribution. Material stays separate from the element. Reason: element code must not hold material laws (D-10).

Plasticity, contact and large displacement are not built yet.

### Geometry and units (planned milestone M2b)

- Text-to-CAD uses Python, with build123d recommended. STEP import goes through gmsh with OpenCASCADE. Reason: Python CAD libraries exist, and a chat can write the script. gmsh already includes OpenCASCADE, so STEP goes through gmsh.
- The case TOML declares units. `fh` refuses on a units mismatch, and refuses a STEP file with no unit record. Reason: a wrong unit gives wrong results without any warning.
- C3D10 (tet10) is the default element. C3D8 (hex8) is only for extruded parts. Reason: tetrahedra mesh any shape, and quadratic elements avoid the bending lock of plain hex8 (a test showed plain hex8 17 percent too small in deflection). Hex8 stays for extruded parts.

### External solvers (agreed 2026-10-13)

- CalculiX (`ccx`) is the first external solver. It runs as a separate program in a new crate, `solver-ccx`. The user installs it. Reason: its input is closest to our `.inp`, and it passed all 6 capability tests (linear, plasticity, contact, large movement, model change, submodel). It stays a separate program because it is GPL.
- Contact in `ccx` uses the penalty method. A rule sets the penalty value, and a sweep in the validation case tests it. Our own Rust reader reads the `ccx` `.frd` result files. Reason: `ccx` supports only penalty contact. A bad penalty value gave a 10 percent error, so a rule and a sweep guard it.
- The built-in solver gets the M5 Newton work. `ccx` serves M6 to M9 first and cross-checks the built-in results. Reason: nonlinear features arrive early, and `ccx` cross-checks the built-in results.

## Documents

- `DESIGN.md`: product design, architecture, solver scope and milestones.
- `AGENTS.md`: rules for the agents that build femahoy.
- [Input keywords and element types](docs/reference/inp.md): accepted input and built-in solver limits.
- [`fh` commands](docs/reference/fh.md): diagnostics and exit codes.
- [Case TOML fields](docs/reference/case-toml.md): schema, measurements, and tolerances.
