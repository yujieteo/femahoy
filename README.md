# femahoy

femahoy is a chat for finite element and CFD analyses. You ask for an analysis, and Claude Code, signed in with your own Claude subscription, does the work on your computer.

## Start

Do these 4 steps on macOS (Apple Silicon) or Linux (x86-64 or ARM64).

1. Install femahoy and the solvers for your user. You need no root and no Nix.

   ```sh
   git clone https://github.com/yujieteo/femahoy
   cd femahoy
   scripts/install.sh
   ```

   The script copies `fh` to `~/.local/bin`. If this folder is not on PATH, the script prints the line to add to your shell start file. See "Install" below.

2. Install Claude Code and sign in with your Claude subscription. If `claude auth status` already shows a login, skip this step.

   ```sh
   curl -fsSL https://claude.ai/install.sh | bash
   claude
   ```

   femahoy reads no credential. The login stays in Claude Code.

3. Start the page.

   ```sh
   fh serve
   ```

   On macOS, `fh serve` opens the page in your browser. On Linux, open the address that it prints. At start, `fh serve` checks that Claude Code starts and has a login. If not, the chat tells you what to do.

   The address is the same at each start: `http://127.0.0.1:8790/?token=TOKEN`. Set `FH_PORT` to use a different port. If the port is in use, `fh serve` stops with an error. The token is in the file `token` of the state folder. Keep the address private, because each person with the address can use the page. To make a new token, remove the file `token` and start `fh serve` again.

   To open the page on a phone, forward a local port of the phone to `127.0.0.1:8790` on the computer through SSH, for example with a local port forward in Termius. On a computer, use `ssh -L 8790:127.0.0.1:8790 HOST`. Then open the same address on the phone. The page accepts only the host `127.0.0.1:8790`, so the local port must also be 8790.

4. Ask for an analysis in the chat. For example: "Build and solve a steel cantilever of 100 x 10 x 10 mm with 1 MPa on the top face".

   The agent writes a CAD script for the part, meshes it with gmsh, writes the solver input and solves it. It solves at least 2 meshes to find the mesh error. The chat shows each command of the agent, and the viewer beside the chat shows the model and the results. At the end, the agent writes 1 HTML report with the results, a hand check and the viewer.

Stop `fh serve` with Ctrl+C. The chat ignores your personal Claude Code setup (`~/.claude/CLAUDE.md`, settings, hooks, plugins, MCP servers and connectors). Thus each person gets the same agent.

## Analysis types

The agent has instructions for each analysis type in this table. Validation cases check each type against a published or measured reference.

| Analysis type | Example case | The case checks |
|---|---|---|
| Structures | [contact-hertz](validation/reports/contact-hertz.html) | The largest contact pressure of a steel ball on a flat steel block, against the Hertz theory |
| CFD | [naca23012-flap](validation/reports/naca23012-flap.html) | The lift and the pitching moment of an airfoil with a plain flap at 3 flap angles, against wind tunnel tests |
| Electromagnetics | [team7-eddy-current](validation/reports/team7-eddy-current.html) | The eddy currents at 50 Hz in an aluminium plate with a hole under a coil, against measurements |
| Acoustics | [acoustics-pulsating-sphere](validation/reports/acoustics-pulsating-sphere.html) | The sound that a pulsating sphere radiates into air at 500 Hz, against the analytical solution |
| Fluid-structure interaction | [turek-hron-fsi1](validation/reports/turek-hron-fsi1.html) | The steady shape of a soft flag behind a cylinder in a channel flow, against the Turek-Hron benchmark |
| Topology optimization | [topology-mbb-beam](validation/reports/topology-mbb-beam.html) | The minimum compliance of the half MBB beam with 50 % of the volume, against a published SIMP code |
| Shape and parameter optimization | [shape-hole-biaxial](validation/reports/shape-hole-biaxial.html) | The hole of minimum stress concentration in a plate under biaxial stress, against the exact optimum |
| Random vibration | [random-vibration-cantilever](validation/reports/random-vibration-cantilever.html) | The RMS tip acceleration of a cantilever on a shaker, against the modal sum of the beam theory |
| Multibody dynamics | [slider-crank](validation/reports/slider-crank.html) | The motion of a slider-crank with a motor, against the closed-form solution |
| Particles (SPH, DEM) | [dam-break-martin-moyce](validation/reports/dam-break-martin-moyce.html) | The surge front of a 2D dam break, against the measurements of Martin and Moyce |
| Explicit dynamics (impact) | [taylor-bar-impact](validation/reports/taylor-bar-impact.html) | The final length and the mushroom radius of a copper bar after an impact on a rigid wall, against a published benchmark |
| Optics and photonics | [optics-mie-cylinder](validation/reports/optics-mie-cylinder.html) | The scatter of a plane wave of light on a dielectric cylinder, against the analytical series solution |
| Antennas | [dipole-king-middleton](validation/reports/dipole-king-middleton.html) | The input impedance and the far field of a centre-fed dipole, against the theory of King and Middleton |

Other structures cases check buckling, plasticity, rubber, cracks, composite laminates, shells, natural frequencies, a sudden load, heat transfer and measured warpage. Other CFD cases check natural convection, conjugate heat transfer, a free surface and a supersonic shock.

`validation/reports/index.html` links the report of each case: the checks, the derivation and a 3D view of the runs. Each report is 1 HTML file that `fh report` wrote. In a clone, open it in a browser. `scripts/validation-reports.py` makes the reports again. In the root of the clone, `fh validate` runs all the cases.

The table in section 6 of `DESIGN.md` gives the solver and the input of each analysis type.

## Your own model

Give `fh run` a CalculiX `.inp` file or the case folder of another analysis type. `fh run` starts the solver itself and needs no daemon. The page of `fh serve` shows the model and the results of each run. `fh mesh` makes the mesh and the sets from a STEP file. `fh report` writes 1 HTML file with a text, the viewer, the results of runs and plots of curves (CSV files of x-y values). The file opens with a double-click. `fh fit` fits a model to a curve, for example the constants of a rubber for CalculiX. `fh --help` lists the commands.

## Install

`scripts/install.sh` needs bash, git, tar, and curl or wget. On macOS, it also needs the Xcode command line tools (`xcode-select --install`). It installs no system package, and it does not install Claude Code.

- The script builds `fh` and the browser viewer from this clone. It installs gmsh and the solver of each analysis type from conda-forge or from the release files of the solver. It builds some solvers from source. The head of the script names each source. The script checks each download with its SHA-256.
- All of it goes to `~/.local/share/femahoy` (`$XDG_DATA_HOME/femahoy`), except the OpenFOAM adapter of preCICE on macOS, which goes to the user folder of OpenFOAM (`~/OpenFOAM`). In the test runs, the folder used 11 to 14 GB. The log of each solver is in the folder `logs` there.
- `~/.local/bin`, or the folder of `--prefix DIR`, gets `fh`, the viewer, `fh-python` (the Python of the CAD scripts, with build123d) and `solvers.env`. `solvers.env` has 1 line `FH_NAME=PATH` for each solver that the script installed. `fh` reads this file at start.
- To update, run `git pull` and `scripts/install.sh` again. A second run builds `fh` again and keeps each solver that has the same version.
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
- `FH_PORT`: the port of `fh serve` on 127.0.0.1. The default is 8790.
- `FH_NO_OPEN`: with a value that is not empty and not `0`, `fh serve` does not open the browser.

## Documents

- `DESIGN.md`: the current rules of the product, each with its reason.
- `AGENTS.md`: rules for the agents that build femahoy.
- [Input keywords and element types](docs/reference/inp.md): the input of the preview and the element types of the page.
- [Check file fields](docs/reference/check-toml.md): measurements and tolerances of a validation case.

femahoy is under the MIT License, see `LICENSE`.
