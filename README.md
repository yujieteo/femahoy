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

For your own model, give `fh run` a CalculiX `.inp` file, an OpenFOAM case folder for CFD (see "Get OpenFOAM" below), a GetDP case folder for electromagnetics or acoustics (see "Get GetDP" below), a preCICE case folder for fluid-structure interaction (see "Get preCICE" below), a topology case folder (`job.inp` and `topology.toml`) for topology optimization with `ccx`, or a Chrono case folder for multibody dynamics (see "Get PyChrono" below). `fh run` starts the solver itself and needs no daemon. The page of `fh serve` shows the model and the results of each run. `fh mesh` makes the mesh and the sets from a STEP file. `fh report` writes 1 HTML file with a text, the viewer and the results of runs. The file opens with a double-click, and the chat uses it for its report.

## Get `ccx`

`fh` starts CalculiX (`ccx`) as a separate program. You install it. `ccx` is also the explicit solver for impact (`*DYNAMIC, EXPLICIT`), so an impact needs no other program.

- Nix (macOS, Linux): `nix build nixpkgs#calculix-ccx -o ccx`, then `export FH_CCX=$PWD/ccx/bin/ccx`.
- Debian, Ubuntu: `apt install calculix-ccx`. This puts `ccx` on PATH.

`fh` uses `$FH_CCX`. If `FH_CCX` is not set, it uses `ccx` on PATH. The validation cases are tested with `ccx` 2.23. The Ubuntu package is older.

## Get OpenFOAM

`fh` starts OpenFOAM as a separate program for CFD. You install it.

- macOS: `brew install gerlero/openfoam/openfoam@2512` installs OpenFOAM.app. This puts `openfoam` on PATH.
- Debian, Ubuntu: install `openfoam2512` from the OpenCFD repository (`https://dl.openfoam.com`, see the step "Install OpenFOAM" in `.github/workflows/ci.yml`). Then `export FH_OPENFOAM=openfoam2512`.

`fh` uses `$FH_OPENFOAM`. If `FH_OPENFOAM` is not set, it uses `openfoam` on PATH. The validation cases are tested with OpenFOAM v2512 of OpenCFD.

## Get GetDP

`fh` starts GetDP as a separate program for electromagnetics and acoustics, and gmsh to mesh the case. You install both.

- gmsh: `brew install gmsh` on macOS, or `nix profile install nixpkgs#gmsh`.
- GetDP on macOS (Apple Silicon): `curl -fsSL https://getdp.info/bin/macOS/getdp-4.0.0-MacOSARMc.tgz | tar xz`, then `export FH_GETDP=$PWD/getdp-4.0.0-MacOSARM/bin/getdp`.
- GetDP on Linux (x86-64): `curl -fsSL https://getdp.info/bin/Linux/getdp-3.5.0-Linux64c.tgz | tar xz`, then `export FH_GETDP=$PWD/getdp-3.5.0-Linux64/bin/getdp`.

`fh` uses `$FH_GETDP`. If `FH_GETDP` is not set, it uses `getdp` on PATH. The validation cases are tested with GetDP 4.0.0 on macOS and 3.5.0 on Linux.

## Get PyChrono

`fh` starts Project Chrono for multibody dynamics: a Python with PyChrono, the Python module of Chrono, runs the `chrono.py` of the case. PyChrono is BSD-3-Clause. It is not on PyPI, so you install it with micromamba from conda-forge.

- micromamba: `brew install micromamba` on macOS, or `nix profile install nixpkgs#micromamba`.
- PyChrono on macOS (Apple Silicon) and Linux (x86-64): `micromamba create -y -p ~/.local/share/pychrono -c conda-forge pychrono=10.0.0 python=3.12`, then `export FH_PYCHRONO=~/.local/share/pychrono/bin/python`. The download is about 550 MB.

`fh` uses `$FH_PYCHRONO`. If `FH_PYCHRONO` is not set, it uses `python3` on PATH. The validation case is tested with PyChrono 10.0.0 on macOS and Linux.

## Get preCICE

`fh` couples OpenFOAM and CalculiX through preCICE for fluid-structure interaction. preCICE (LGPL) and its 2 adapters (GPL) are separate programs. You install them and OpenFOAM (see "Get OpenFOAM").

- macOS (Apple Silicon), with Nix, in a clone of this repository:

  ```sh
  nix build nixpkgs#precice -o precice
  nix build nixpkgs#pkg-config -o pkg-config
  nix build -f scripts/ccx-precice.nix -o ccx-precice
  export FH_CCX_PRECICE=$PWD/ccx-precice/bin/ccx_preCICE
  curl -fsSL https://github.com/precice/openfoam-adapter/releases/download/v1.4.0/openfoam-adapter-1.4.0-OpenFOAMv1812-v2606-newer.tar.gz | tar xz
  cd openfoam-adapter-1.4.0
  PATH=$PWD/../pkg-config/bin:$PATH PKG_CONFIG_PATH=$PWD/../precice/lib/pkgconfig openfoam ./Allwmake
  ```

- Ubuntu 24.04: install the packages of preCICE and of the CalculiX adapter from their GitHub releases, and build the OpenFOAM adapter with `openfoam2512 ./Allwmake`. The step "Install preCICE" in `.github/workflows/ci.yml` shows the commands. The package puts `ccx_preCICE` on PATH.

`./Allwmake` puts the OpenFOAM adapter in the user library folder of OpenFOAM (`$FOAM_USER_LIBBIN`). The fluid solver loads it from there. `fh` uses `$FH_CCX_PRECICE`. If `FH_CCX_PRECICE` is not set, it uses `ccx_preCICE` on PATH. The validation case is tested with preCICE 3.4.1, the CalculiX adapter 2.20.2 and the OpenFOAM adapter 1.4.0.

## Environment variables

- `FH_GMSH`: the gmsh program of `fh mesh`, and of `fh run` and `fh validate` for a GetDP case.
- `FH_CCX`: the `ccx` program of `fh run` and `fh validate`.
- `FH_OPENFOAM`: the `openfoam` program of `fh run` and `fh validate`. It runs each program of OpenFOAM as `$FH_OPENFOAM PROGRAM ARGS`.
- `FH_GETDP`: the `getdp` program of `fh run` and `fh validate`.
- `FH_PYCHRONO`: the Python with PyChrono of `fh run` and `fh validate`.
- `FH_CCX_PRECICE`: the `ccx_preCICE` program (CalculiX with the preCICE adapter) of `fh run` and `fh validate`.
- `FH_STATE`: the state folder of `fh run`, `fh report` and `fh serve`.
- `FH_VIEWER`: the folder of the wasm viewer of `fh report` and `fh serve`.
- `FH_NO_OPEN`: a value that is not empty and not `0` stops `fh serve` from opening the browser.

## Documents

- `DESIGN.md`: the current rules of the product, each with its reason.
- `AGENTS.md`: rules for the agents that build femahoy.
- [Input keywords and element types](docs/reference/inp.md): the input of the preview and the element types of the page.
- [Check file fields](docs/reference/check-toml.md): measurements and tolerances of a validation case.

femahoy is under the MIT License, see `LICENSE`.
