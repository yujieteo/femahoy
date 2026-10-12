# femahoy

femahoy is a finite element workflow in Rust. LLM agents and humans use it together.

The engine is the `fh` command-line program. The main interface is a visual chat. A 3D viewport and a terminal sidebar are beside the chat.

The chat plans the work and gives tasks to teammates. The user interface shows each teammate while it runs.

The input is `.inp` text, heavily inspired by Abaqus input conventions. Agent-facing output is TOON. Results are VTU files, so ParaView can open them.

## Status

Agreed design, before implementation. The name is provisional. The name depends on trademark, package-name and domain checks.

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
| `viewer` | eframe app, wgpu mesh and contour renderer, terminal widget, chat panel, teammate tree | 15k |
| **Total** | | **85k** |

## Planned platforms

1. Web (wasm) first. The user installs `fh`, runs `fh serve`, and the user interface opens in the browser.
2. Native macOS second.
3. Windows third. It uses the plain PTY backend until herdr Windows support is stable.

The product is distributed as one `fh` binary, through Homebrew and as a direct download.

## Documents

- `DESIGN.md`: product design, architecture, solver scope and milestones.
- `AGENTS.md`: rules for the agents that build femahoy.
