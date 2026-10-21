# femahoy

femahoy is a finite element and CFD workflow in Rust. LLM agents and humans use it together.

## Start

Run these 2 commands in a clone of this repository, on macOS or Linux:

```sh
scripts/install.sh
fh serve
```

1. `scripts/install.sh` builds `fh` and the browser viewer, and copies them to `~/.local/bin`. Use `--prefix DIR` for another folder. The script first checks the tools (Rust with rustup, the `wasm32-unknown-unknown` target, `wasm-bindgen-cli` and uv). If a tool is missing, the script stops and shows the command that installs it. Each solve also needs CalculiX (`ccx`, see "Get `ccx`" below).
2. `fh serve` starts a local daemon and opens the page in the browser (on a system that is not macOS, it only prints the address). Write a request in the chat of the page, for example "Build and solve a steel cantilever of 100 x 10 x 10 mm with 1 MPa on the top face". The agent makes the part, meshes it with gmsh (`brew install gmsh`), writes the CalculiX input and solves it. The viewer of the page shows the results. The chat needs Claude Code (`claude`) with a login. The chat ignores your personal Claude Code setup (`~/.claude/CLAUDE.md`, settings, hooks, plugins, MCP servers and connectors). Stop the daemon with Ctrl+C.

For your own model, give `fh run` a CalculiX `.inp` file, or an OpenFOAM case folder for CFD (see "Get OpenFOAM" below). `fh run` starts the solver itself and needs no daemon. The page of `fh serve` shows the model and the results of each run. `fh mesh` makes the mesh and the sets from a STEP file. `fh report` writes 1 HTML file with a text, the viewer and the results of runs. The file opens with a double-click, and the chat uses it for its report.

## Get `ccx`

`fh` starts CalculiX (`ccx`) as a separate program. You install it.

- Nix (macOS, Linux): `nix build nixpkgs#calculix-ccx -o ccx`, then `export FH_CCX=$PWD/ccx/bin/ccx`.
- Debian, Ubuntu: `apt install calculix-ccx`. This puts `ccx` on PATH.

`fh` uses `$FH_CCX`. If `FH_CCX` is not set, it uses `ccx` on PATH. The validation cases are tested with `ccx` 2.23. The Ubuntu package is older.

## Get OpenFOAM

`fh` starts OpenFOAM as a separate program for CFD. You install it.

- macOS: `brew install gerlero/openfoam/openfoam@2512` installs OpenFOAM.app. This puts `openfoam` on PATH.
- Debian, Ubuntu: install `openfoam2512` from the OpenCFD repository (`https://dl.openfoam.com`, see the step "Install OpenFOAM" in `.github/workflows/ci.yml`). Then `export FH_OPENFOAM=openfoam2512`.

`fh` uses `$FH_OPENFOAM`. If `FH_OPENFOAM` is not set, it uses `openfoam` on PATH. The validation cases are tested with OpenFOAM v2512 of OpenCFD.

## Environment variables

- `FH_GMSH`: the gmsh program of `fh mesh`.
- `FH_CCX`: the `ccx` program of `fh run` and `fh validate`.
- `FH_OPENFOAM`: the `openfoam` program of `fh run` and `fh validate`. It runs each program of OpenFOAM as `$FH_OPENFOAM PROGRAM ARGS`.
- `FH_STATE`: the state folder of `fh run`, `fh report` and `fh serve`.
- `FH_VIEWER`: the folder of the wasm viewer of `fh report` and `fh serve`.
- `FH_NO_OPEN`: a value that is not empty and not `0` stops `fh serve` from opening the browser.

## Documents

- `DESIGN.md`: the current rules of the product, each with its reason.
- `AGENTS.md`: rules for the agents that build femahoy.
- [Input keywords and element types](docs/reference/inp.md): the input of the preview and the element types of the page.
- [Check file fields](docs/reference/check-toml.md): measurements and tolerances of a validation case.

femahoy is under the MIT License, see `LICENSE`.
