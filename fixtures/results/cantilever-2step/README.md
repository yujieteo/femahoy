# cantilever-2step

Results of `cantilever-2step.inp` (the `cantilever.inp` beam with a second step that doubles the tip load). The viewer tests read them.

The native solver `fh-solve` made these files. The owner removed it on 2026-10-19. Main commit `08a97c2` has it. `fh-solve-ccx` writes no text results, so keep these files. They were made with:

```
fh-solve cantilever-2step.inp run --formats text,vtu
cp run/results.pvd run/step-*.vtu run/step-*.txt .
```

Do not edit the files by hand. The tests compare the VTU values with the text results.
