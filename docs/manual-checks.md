# Manual platform checks

GitHub Actions runs the x86-64 Linux checks and an aarch64 macOS job that builds the workspace and runs the workspace tests. It runs no release build, memory test, convergence check or CAD mesh check.
Run the scripts below from any directory. Each script uses the repository root and exits non-zero on a failure.

## Apple Silicon macOS

Install Rust with Cargo, rustfmt and clippy. Install the `aarch64-apple-darwin` Rust target.
Run `scripts/install.sh` for `fh-python`, the Python with build123d. Install a separate gmsh executable as described in [the CAD recipe](cad-recipe.md).
The script does not install these tools.

```sh
rustup component add rustfmt clippy
rustup target add aarch64-apple-darwin
scripts/check-mac-e2e.sh
```

The script uses `fh-python` when it is on `PATH`, and `python3` when it is not. Set `PYTHON` to another Python executable with build123d if necessary.
Set `FH_GMSH` to a gmsh executable outside `PATH` if necessary.

```sh
PYTHON=/path/to/venv/bin/python FH_GMSH=/path/to/gmsh scripts/check-mac-e2e.sh
```

The script requires Apple Silicon macOS. It prints a short pass or fail table and stops at the first failure.
It keeps logs, build files, the CAD mesh and results in the temporary directory that it prints.
Remove that directory when you no longer need the evidence.

The checks include:

- Format, clippy and workspace tests for the aarch64 target.
- Release builds of `fh` and `fh-solve`.
- `fh validate` and `fh validate --convergence validation/cases` on the supplied cases.
- The existing build123d bar recipe, STEP unit check, `fh mesh` and mesh quality check.
- Separate solves of the patch-test, cantilever and coarse Lamé `.inp` fixtures, with stdin open until each solver exits.
- Result status, PVD datasets, VTU mesh counts and field names, and nonempty text results.

**Known gap:** the CAD mesh does not supply the solver input.
The `.inp` fixtures supply separate models. The script states this gap in its output.
These checks do not prove a complete CAD-to-solver path or the accuracy of a new CAD model.
The validation cases check numerical results against their references.
