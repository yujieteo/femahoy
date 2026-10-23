# femahoy

femahoy is a finite element and CFD workflow in Rust. LLM agents and humans use it together.

## Start

Run these 2 commands in a clone of this repository, on macOS (Apple Silicon) or Linux (x86-64 or ARM64):

```sh
scripts/install.sh
fh serve
```

1. `scripts/install.sh` installs femahoy for your user, with no root and no Nix. It builds `fh` and the browser viewer, installs gmsh and each solver, and copies `fh` to `~/.local/bin`. Use `--prefix DIR` for another folder. See "Install" below.
2. `fh serve` starts a local daemon and opens the page in the browser (on a system that is not macOS, it only prints the address). Write a request in the chat of the page, for example "Build and solve a steel cantilever of 100 x 10 x 10 mm with 1 MPa on the top face". The agent makes the part, meshes it with gmsh, writes the CalculiX input and solves it. The viewer of the page shows the results. The chat needs Claude Code (`claude`), signed in with your Claude subscription. If `claude` is not installed or has no login, the chat of the page tells you how to install it and sign in. The chat ignores your personal Claude Code setup (`~/.claude/CLAUDE.md`, settings, hooks, plugins, MCP servers and connectors). Stop the daemon with Ctrl+C.

For your own model, give `fh run` a CalculiX `.inp` file or the case folder of another analysis type. The table in section 6 of `DESIGN.md` gives the solver and the input of each analysis type. `fh run` starts the solver itself and needs no daemon. The page of `fh serve` shows the model and the results of each run. `fh mesh` makes the mesh and the sets from a STEP file. `fh report` writes 1 HTML file with a text, the viewer, the results of runs and plots of curves (CSV files of x-y values). The file opens with a double-click, and the chat uses it for its report. `fh fit` fits a model to a curve, for example the constants of a rubber for CalculiX.

`validation/reports/index.html` links 1 report of `fh report` for each validation case: the checks, the derivation and the runs. `scripts/validation-reports.py` makes them again.

## Install

`scripts/install.sh` needs bash, git, tar, and curl or wget. On macOS, it also needs the Xcode command line tools (`xcode-select --install`). It installs no system package.

- micromamba installs Rust, gmsh, CalculiX, PyChrono and Meep from conda-forge, and OpenFOAM on Linux x86-64. On macOS, the script downloads OpenFOAM.app from `github.com/gerlero/openfoam-app`. It downloads GetDP from `getdp.info`. It builds preCICE with its CalculiX and OpenFOAM adapters, LAMMPS with its SPH package, and openEMS, from source. It checks each download with its SHA-256.
- All of it goes to `~/.local/share/femahoy` (`$XDG_DATA_HOME/femahoy`), except the OpenFOAM adapter of preCICE on macOS, which goes to the user folder of OpenFOAM (`~/OpenFOAM`). In the test runs, the folder used 11 to 14 GB. A second run builds `fh` again and keeps each solver that has the same version. The log of each solver is in the folder `logs` there.
- The script writes `solvers.env` beside `fh`: 1 line `FH_NAME=PATH` for each solver that it installed. `fh` reads this file at start.
- At the end, the script names each solver that it did not install, with the analysis types that it disables. On Linux ARM64, OpenFOAM and GetDP have no build, so CFD, fluid-structure interaction, electromagnetics and acoustics are not available there.

To use your own solver, for example from Nix, set its variable (see "Environment variables") before you run the script. The script then does not install that solver, and `fh` uses the variable, because a variable of the environment has priority over `solvers.env`. `scripts/ccx-precice.nix` builds `ccx_preCICE` with Nix. preCICE also needs its OpenFOAM adapter, which the script builds only for the OpenFOAM that it installs.

## Environment variables

A solver variable has priority over its line in `solvers.env`. With neither, `fh` uses the program of the list below on PATH, or `python3` for a Python solver.

- `FH_GMSH`: the gmsh program of `fh mesh`, and of `fh run` and `fh validate` for a GetDP case.
- `FH_CCX`: the `ccx` program of `fh run` and `fh validate`.
- `FH_OPENFOAM`: the `openfoam` program of `fh run` and `fh validate`. It runs each program of OpenFOAM as `$FH_OPENFOAM PROGRAM ARGS`.
- `FH_GETDP`: the `getdp` program of `fh run` and `fh validate`.
- `FH_PYCHRONO`: the Python with PyChrono of `fh run` and `fh validate`.
- `FH_MEEP`: the Python with Meep of `fh run` and `fh validate`.
- `FH_CCX_PRECICE`: the `ccx_preCICE` program (CalculiX with the preCICE adapter) of `fh run` and `fh validate`.
- `FH_LAMMPS`: the `lmp` program of `fh run` and `fh validate`.
- `FH_OPENEMS`: the Python with openEMS of `fh run` and `fh validate`.
- `FH_STATE`: the state folder of `fh run`, `fh report` and `fh serve`.
- `FH_VIEWER`: the folder of the wasm viewer of `fh report` and `fh serve`.
- `FH_NO_OPEN`: a value that is not empty and not `0` stops `fh serve` from opening the browser.

## Documents

- `DESIGN.md`: the current rules of the product, each with its reason.
- `AGENTS.md`: rules for the agents that build femahoy.
- [Input keywords and element types](docs/reference/inp.md): the input of the preview and the element types of the page.
- [Check file fields](docs/reference/check-toml.md): measurements and tolerances of a validation case.

femahoy is under the MIT License, see `LICENSE`.
