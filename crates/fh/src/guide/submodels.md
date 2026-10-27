# Submodels

For the peak stress at a small detail of a large part, solve a coarse global model of the part, then a fine submodel of the region at the detail. The submodel gets the displacement of the global model on its cut boundary.

1. Global model: write `global.inp`, a complete model with `*NODE FILE, OUTPUT=3D` and `U` in its step. Without `OUTPUT=3D`, a 2D submodel stopped with `*ERROR in dsort`.
2. Submodel: write `job.inp` beside `global.inp`, with a fine mesh of the region and a node set `CUT` of the cut boundary. Add `*SUBMODEL, TYPE=NODE, INPUT=global.frd` with the line `CUT` before the first step. In the step, write `*BOUNDARY, SUBMODEL, STEP=1` with the line `CUT, 1, 2` (`CUT, 1, 3` in 3D), and no other load at the cut.
3. Solve: `fh run job.inp`. `fh` copies `global.inp` and its `*INCLUDE` files, solves `global.inp`, then solves `job.inp` in the same folder.
4. Cut: put the cut far from the detail, for example at r = 2a for a hole of radius a. A cut at r = 1.5a gave an error of 1 %. Near the cut, the submodel has the error of the global model: read its results away from the cut.
5. Check: the submodel is only as good as the displacement of the global model at the cut. Solve 2 global meshes and compare the result of the submodel.

This setup agrees within 0.03 % with the Kirsch solution for the stress at a hole in a plate in tension, from a global model of 16 elements that is 4.5 % low (the validation case `submodel-kirsch-hole`).
