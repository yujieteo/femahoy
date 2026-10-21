# particles-2step

Results of a LAMMPS case with 2 particles. From time 0 to time 0.5, each particle moves 0.5 in x. Each particle is a cube with a side of 0.5. The viewer tests read them.

`fh run` with the fake `lmp` of `crates/fh/tests/run.rs` (`FAKE_LMP`) made these files. Put the script in a file `lmp`, then `fh run` prints the results folder `OUT`:

```
echo "run 10" > case/case.lmp
FH_LAMMPS=./lmp fh run case
cp OUT/results.pvd OUT/step-*.vtu .
```

Do not edit the files by hand.
