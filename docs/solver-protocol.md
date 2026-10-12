# Subprocess solver protocol v1

The runner starts `exe <input.inp> <run-dir>` in the run folder.

The solver must:

1. Print `fh-solver-protocol 1` as the first line on stdout.
2. Write `results.pvd`, each VTU file that the PVD lists, and `summary.txt` in the run folder.
3. Exit with code 0 on success. On failure, exit with a non-zero code and print the reason on stderr.

The runner stops a solver that exceeds the time limit or the memory limit.
The memory check covers the solver process, not the processes that it starts.
The defaults are 10 minutes and 4 GiB. Both values are provisional.

`summary.txt` is provisional plain text. The TOON summary schema is an open design item.
The runner does not parse the file.
Results from a subprocess solver are always unvalidated.
