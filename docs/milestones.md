# Milestones

This table lists the milestones from `DESIGN.md` section 13.
Only M0 is done.

| Milestone | Content | Status |
|---|---|---|
| M0 Skeleton | Workspace with all crates. `fh --help`. CI: fmt, clippy, test, `tokei` budget check. | Done |
| M1 Parser and harness | `inp` reads the keyword subset for linear static. Gmsh `.msh` import. Validation harness with the first three fixtures (patch test, cantilever, Lamé). TOON summary schema v1. Fixture schema. | Not started |
| M2 Linear solver | `C3D8I`, `C3D8`, `C3D10`. DOF elimination. `faer` Cholesky. Boundary conditions, loads, `*COUPLING`, `*EQUATION`, `*MPC`. VTU and PVD output. Subprocess protocol v1. | Not started |
| M3 Daemon and viewer | `fh serve` with token and `Origin` checks. wasm viewer: mesh, outlines, deformed shape, one contour field, step selector. Terminal sidebar. Both pane backends. | Not started |
| M4 Chat and agents | Leader in headless mode in the chat. Teammates in panes. Teammate tree. `femahoy.toml`. `fh init` installs skills. Both connection paths. | Not started |
| M5 Contact | Newton with load stepping and cutback. `C3D10M`. Augmented Lagrange node-to-surface contact. | Not started |
| M6 Plasticity | J2 with isotropic hardening. | Not started |
| M7 Large displacement | `NLGEOM=YES`, corotational update. | Not started |
| M8 Birth and death | `*MODEL CHANGE`. | Not started |
| M9 Submodelling | `*SUBMODEL`, `*BOUNDARY, SUBMODEL`. | Not started |
| M10 Native macOS | Native build. | Not started |
| M11 Windows | Windows build, plain PTY backend. | Not started |
