# cantilever-2step

Results of `cantilever-2step.inp`: a 10 x 1 x 1 mm steel beam of 24 `C3D10` elements, fixed at x = 0. Step 1 puts a load in z on the 9 nodes of the tip x = 10. Step 2 doubles the load. The viewer tests read them.

`fh-solve-ccx` with `ccx` 2.23 made these files:

```
fh-solve-ccx cantilever-2step.inp run
cp run/results.pvd run/step-*.vtu .
```

Do not edit the files by hand.
