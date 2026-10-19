# femahoy

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

## Start

Run these 2 commands in a clone of this repository, on macOS or Linux:

```sh
scripts/install.sh
fh serve
```

1. `scripts/install.sh` builds `fh`, `fh-solve-ccx` and the browser viewer, and copies them to `~/.local/bin`. Use `--prefix DIR` for another folder. The script first checks the tools (Rust with rustup, the `wasm32-unknown-unknown` target, `wasm-bindgen-cli` and uv). If a tool is missing, the script stops and shows the command that installs it. Each solve also needs CalculiX (`ccx`, [docs/ccx.md](docs/ccx.md)).
2. `fh serve` starts a local daemon and opens the page in the browser. Write a request in the chat of the page, for example "Build and solve a steel cantilever of 100 x 10 x 10 mm with 1 MPa on the top face". The agent makes the part, meshes it with gmsh (`brew install gmsh`) and solves it. The viewer of the page shows the results. The chat needs Claude Code (`claude`) with a login ([docs/chat.md](docs/chat.md)). Stop the daemon with `fh serve --stop`.

For your own model, give `fh run` an `.inp` file or a case folder (`case.toml` and one STEP file). The [quickstart](docs/quickstart.md) shows each step, and [`fh` commands](docs/reference/fh.md) gives the options and the exit codes.

## Product

The engine is the `fh` command-line program. The main interface is a visual chat. A 3D viewport is beside the chat.

The chat has 1 focused agent. It plans and does the work.

The input is `.inp` text, heavily inspired by Abaqus input conventions. Agent-facing output is TOON. Results are VTU files, so ParaView can open them.

The solver is CalculiX (`ccx`), through the `fh-solve-ccx` adapter. The owner removed the native solver `fh-solve` on 2026-10-19. An own solver is a main advantage later, but not now. Main commit `08a97c2` has the native solver.

## Status

The roadmap and the status of each item are in [docs/milestones.md](docs/milestones.md).

## Licence

The code is licensed under AGPL-3.0.

## Documents

- [Quickstart](docs/quickstart.md): the install, `fh run`, and the STEP, mesh, solver and validation commands, with output from actual runs.
- `DESIGN.md`: product design, architecture, crates, solver scope and the decision log (section 15).
- `AGENTS.md`: rules for the agents that build femahoy.
- [Input keywords and element types](docs/reference/inp.md): accepted input and solver limits.
- [`fh` commands](docs/reference/fh.md): diagnostics and exit codes.
- [Case TOML fields](docs/reference/case-toml.md): schema, measurements, and tolerances.
