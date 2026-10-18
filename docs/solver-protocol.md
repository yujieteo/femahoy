# Subprocess solver protocol v2

The runner starts `exe <input.inp> <run-dir>` with the run folder as the working directory.
A relative `exe` path with a folder resolves from the runner's working directory, not from the run folder.
The runner supports protocol v2 only.

## Run folder

- The runner clears the run folder before a run, but only if the folder has the marker file `.fh-run`.
- If the folder is not empty and has no marker, the runner stops with an error and deletes nothing.
- The runner writes the marker on the first run.

## Solver output (stdout, one line each)

1. The first line is `fh-solver-protocol 2`.
2. Event lines follow:
   - `progress step=<i>/<n> phase=<name> fraction=<0..1>`
   - `estimate bytes=<n>` is the peak memory that the solver expects. The runner stops the solver if the value is above the memory limit.
   A `progress` line with a bad step or a fraction outside 0 to 1 is a log line.
3. Any other line is a log line. A line longer than 64 KiB is an error and the runner stops the solver.

`fh-solve-ccx` (`docs/ccx.md`) writes only the first line. It also takes `--formats vtu`. The runner does not pass this option.

## Cancel

The runner sends the line `cancel` on stdin.
The runner also sets the environment variable `FH_CANCEL_ON_EOF=1`. Then the end of stdin also means cancel, so a solver stops when the runner is gone.
Without `FH_CANCEL_ON_EOF=1`, the end of stdin does not stop the solver. Thus one shell command with no input, for example `fh-solve-ccx model.inp out < /dev/null`, finishes the solve.
The solver must stop, and exit with a non-zero code. A cancelled run is always an error, also if the solver exits with code 0.
If the solver is still alive after 5 seconds, the runner kills the whole process group.

## Result files

On success the solver writes these files in the run folder, and exits with code 0:

- `results.pvd` and each result file that it lists.
- `summary.toon`, a TOON document with the first line `schema: fh.run` and a `status:` line (schema in the TOON schema document, sections 3.5 and 3.8).

Each PVD path must be relative, must not contain `..`, and must stay inside the run folder.
On failure the solver exits with a non-zero code and prints the reason on stderr.

## Result contract

`fh validate` reads these arrays in each VTU file. `fh-solve-ccx` uses the shared code in `fh_io::result` for the ids, the set flags and the rule for `S`.

- `NODE_ID` on the points and `ELEMENT_ID` on the cells, in model order.
- `NSET_<name>` on the points and `ELSET_<name>` on the cells, one for each set of the model. The value is 1 for a member of the set and 0 for all other entries.
- `U` and `RF` on the points.
- `S` on the cells, in the order `[xx, yy, zz, xy, yz, xz]`. `S` is the volume mean of the integration point stress. The weight of each point is its integration weight times its Jacobian determinant.
- `MISES` on the cells, from `S`.

## Limits

- There is no default time limit, because a large model can solve for hours. A cancel stops a run.
- The default memory limit is 85% of the machine RAM. If the RAM is not known, there is no limit.
- A validation case can set its own limits with `time_limit_s` and `memory_limit_mib`.
- The error at a limit tells the user to use a coarser mesh. A solver that a signal stops, with no error text, gets the same advice, because a full memory is the usual cause.
- The runner starts the solver in its own process group, and kills the whole group on a timeout, a memory limit or a cancel.
- The memory check sums the resident memory of the whole group (Linux: `/proc`, macOS: `ps`, Windows: the process).
- Linux: the runner also sets a hard address space limit (limit + 256 MiB). Windows: a job object with a process memory limit.
- macOS has no hard limit that the kernel enforces. Only the probe applies.
- If the probe fails, the runner stops the solver with an error.

Results from a subprocess solver are always unvalidated.
