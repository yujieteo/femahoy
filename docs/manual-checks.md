# Manual platform checks

GitHub Actions runs the x86-64 Linux checks and an aarch64 macOS job that builds the workspace and runs the workspace tests.
The workflow `no-avx` runs `scripts/check-no-avx.sh` (below) on x86-64 Linux under an emulator. It runs on each push to `main`, when a person starts it, and on a pull request that changes the files of the job.
No job runs the memory test or a convergence check.
Run the scripts below from any directory. Each script uses the repository root and exits non-zero on a failure.

## Apple Silicon macOS

Install Rust with Cargo, rustfmt and clippy. Install the `aarch64-apple-darwin` Rust target.
Install Python with build123d and a separate gmsh executable as described in [the CAD recipe](cad-recipe.md).
The script does not install these tools.

```sh
rustup component add rustfmt clippy
rustup target add aarch64-apple-darwin
scripts/check-mac-e2e.sh
```

Set `PYTHON` to the Python executable of your build123d environment if necessary.
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

## x86-64 Linux without AVX

Use an x86-64 Linux host with Cargo, a C compiler, `qemu-x86_64` user-mode emulation and GNU time.
Install Python with build123d and gmsh as in [the CAD recipe](cad-recipe.md).
The workflow `.github/workflows/no-avx.yml` gives the packages of an Ubuntu host. On Ubuntu, install QEMU, GNU time and the compiler with `sudo apt-get install qemu-user time build-essential`.
The Rust target is `x86_64-unknown-linux-gnu`.

```sh
rustup target add x86_64-unknown-linux-gnu
scripts/check-no-avx.sh
```

The script first runs an AVX instruction under QEMU with the Nehalem CPU model.
This probe must fail with SIGILL (exit code 132). A different exit code stops the check.
Then the script builds release `fh` and `fh-solve` with these flags:

```text
-C target-cpu=x86-64 -C target-feature=+sse4.2,-avx,-avx2
```

All later steps run under the same Nehalem model. QEMU runs each program in a separate wrapper, because the child of an emulated process runs without the emulator.

1. `fh validate`, with every solver subprocess.
2. The CLI workflow of `scripts/check-workflow.sh`: build123d (Python), `fh`, gmsh and `fh-solve`.
3. `scripts/check-threads.py`: 2 inputs of the workflow and 2 generated boxes, at 1, 2, 4 and 6 threads (`FH_THREADS`). Each input runs to the end, and also with a kill after the first restart file and then `--restart`. Every file of the run folder must have the bytes of the full run at 1 thread. Only the summary lines `wall_s` and `run_id` can differ.
4. The unit test `step::tests::the_matrix_has_the_same_bits_for_any_thread_count` of `fh-core-solver`, built with the same flags. The solver writes no matrix file, so this test compares the matrix values.
5. A record of the CPU features that the external programs find (`numpy.show_runtime()`) or declare (`readelf -n`).

An illegal instruction, a failed step or a difference in the bytes makes the script fail.
With GNU time, the script records the elapsed time and the peak memory of each step. The values are emulator values, not hardware values.
The build uses a new temporary directory. The script removes that directory when it exits.
Set `FH_CHECK_OUT` to a new folder to keep the logs and the tables. Set `PYTHON` and `FH_GMSH` as for the Mac script.

On a Mac, use an amd64 Linux container with these tools and mount the repository inside it.
Run the same script inside the container. The script does not install or start a container.

Neither script measures the 1-million-element memory target. The Mac script includes the existing tests of results across thread counts.
An emulator is not a real CPU without AVX. A run on real hardware remains future manual work.
