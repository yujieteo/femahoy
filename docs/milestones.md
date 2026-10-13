# Milestones

This table lists the milestones from `DESIGN.md` section 13.
M0 and M1 are done. M2 is in review (PR 10). M3 and later are not built yet.
The decision IDs (D-xx) are in `DESIGN.md` section 17.

| Milestone | Content | Status |
|---|---|---|
| M0 Skeleton | Workspace with all crates. `fh --help`. CI: fmt, clippy, test, `tokei` budget check. | Done |
| M1 Parser and harness | `inp` reads the keyword subset for linear static. Gmsh `.msh` import. Validation harness with the first three fixtures (patch test, cantilever, Lamé). TOON summary schema v1. Fixture schema. | Done |
| M2 Linear solver | `C3D8I`, `C3D8`, `C3D10`. DOF elimination. Sparse solver (`faer` Cholesky) that fits 1 million `C3D8` elements in 16 GB (D-06). Boundary conditions, loads, `*COUPLING`, `*EQUATION`, `*MPC`. Restart that is safe against partial files (D-09). Three result formats: plain text, VTU with PVD, legacy VTK (D-04). `Material` trait with elasticity, and steps that keep the later analysis types possible (D-10). Subprocess protocol v2. | In review. All code is written and `fh validate` passes the 3 cases through the solver (cantilever, Lamé cylinder, patch test). Not Done until the validator accepts PR 10 and the manufactured-solution rates are reported. |
| M3 Daemon and viewer | `fh serve` with token and `Origin` checks. wasm viewer: mesh, outlines, deformed shape, one contour field, step selector. Terminal sidebar. Both pane backends. | Not started |
| M4 Chat and agents | Leader in headless mode in the chat. Teammates in panes. Teammate tree. `femahoy.toml`. `fh init` installs skills. Both connection paths. | Not started |
| M5 Nonlinear solver | Newton with load stepping and cutback. A linear fallback for the same model. Assembly stays ready for parallel work. `C3D10M`. | Not started |
| M6 Plasticity and contact | J2 plasticity with isotropic hardening, through the `Material` trait. Contact is a separate contribution to the system, not part of the element. Augmented Lagrange node-to-surface contact. | Not started |
| M7 Large displacement | `NLGEOM=YES` through the deformation gradient, so that elements and materials use one path. | Not started |
| M8 Birth and death | `*MODEL CHANGE`. | Not started |
| M9 Submodelling | `*SUBMODEL`, `*BOUNDARY, SUBMODEL`. | Not started |
| M10 Native macOS | Native build. | Not started |
| M11 Windows | Windows build, plain PTY backend. | Not started |

Later, with no milestone number yet: a GPU backend (Metal first, D-08), `fh mesh`, and a mesher written in Rust (D-02). None of these is built yet.
