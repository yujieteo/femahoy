# femahoy

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

## Start

Run these 2 commands in a clone of this repository, on macOS or Linux:

```sh
scripts/install.sh
fh run --example bar-pressure
```

1. `scripts/install.sh` builds `fh`, `fh-solve` and the browser viewer, and copies them to `~/.local/bin`. Use `--prefix DIR` for another folder. The script first checks the tools (Rust with rustup, the `wasm32-unknown-unknown` target and `wasm-bindgen-cli`). If a tool is missing, the script stops and shows the command that installs it.
2. `fh run --example bar-pressure` works in any folder. It meshes a steel bar with gmsh (`brew install gmsh`), writes the solver input, and solves it on a local daemon. Then it opens the results in the browser.

The browser shows the bar with the contour of the von Mises stress. The legend shows `MISES (MPa), per element` and one band, `constant 100.0`, because a pressure of 100 MPa pulls the end of the bar. The daemon stays alive, so the page keeps the results. Stop it with `fh serve --stop`.

For your own model, give `fh run` an `.inp` file or a case folder (`case.toml` and one STEP file). The [quickstart](docs/quickstart.md) shows each step, and [`fh` commands](docs/reference/fh.md) gives the options and the exit codes.

## Product

The engine is the `fh` command-line program. The main interface is a visual chat. A 3D viewport and a terminal sidebar are beside the chat.

The chat plans the work and gives tasks to teammates. The user interface shows each teammate while it runs.

The input is `.inp` text, heavily inspired by Abaqus input conventions. Agent-facing output is TOON. Results are VTU files, so ParaView can open them.

## Status

The roadmap and the status of each item are in [docs/milestones.md](docs/milestones.md). The name is provisional. The name depends on trademark, package-name and domain checks.

## Licence

The code is licensed under AGPL-3.0.

A commercial licence is for sale to companies that cannot accept AGPL.

## Documents

- [Quickstart](docs/quickstart.md): the install, `fh run`, and the STEP, mesh, solver and validation commands, with output from actual runs.
- `DESIGN.md`: product design, architecture, crates, solver scope and the decision log (section 17).
- `AGENTS.md`: rules for the agents that build femahoy.
- [Input keywords and element types](docs/reference/inp.md): accepted input and built-in solver limits.
- [`fh` commands](docs/reference/fh.md): diagnostics and exit codes.
- [Case TOML fields](docs/reference/case-toml.md): schema, measurements, and tolerances.
