# Changelog

Each entry is 1 release of femahoy. The top entry is the next or the latest release. `scripts/release.sh` uses its text as the notes of the GitHub release.

## 0.1.0

The first release. femahoy is a chat for finite element and CFD analyses. Claude Code, with your own Claude subscription, does the work on your computer.

- `fh serve` shows the chat and a 3D viewer in the browser, on 127.0.0.1 with a token. The page fits phone screens.
- The agent writes a CAD script, meshes the part with gmsh, solves at least 2 meshes and writes 1 HTML report with the results and a hand check.
- `fh run` solves a CalculiX input or a case of OpenFOAM, GetDP, preCICE, Project Chrono, LAMMPS, Meep or openEMS. The analysis types are structures, CFD, electromagnetics, acoustics, fluid-structure interaction, topology and shape optimization, random vibration, multibody dynamics, particles, explicit dynamics, optics and antennas.
- The built-in solver `fh-fem` solves heat, thermal stress, DC conduction, natural frequencies, frequency-domain electromagnetics, periodic unit cells and transmission line cross-sections with no external solver. It also runs in the browser.
- `fh mesh`, `fh report`, `fh fit`, `fh guide` and `fh validate` mesh a STEP file, write a report, fit a model to a curve, print a guide for the agent and run the validation cases.
- 69 validation cases check the solvers against published or measured references. 5 of them also run offline in 1 HTML file each.
- `scripts/install.sh` installs `fh` and the solvers for the user on macOS (Apple Silicon) and Linux, with no root.
- `fh --version` prints the version.
