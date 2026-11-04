# Shape and parameter optimization

To make the peak of a field a minimum, find the shape or the dimensions. Write an optimization case folder and solve it with `fh run CASE`. An example is the stress at a hole. For another objective or a constraint, write a loop in `fh-python` that runs `fh run` for each design.

1. `mesh.geo`: the gmsh geometry. Give each design variable with `DefineConstant[ name = value ];`. A plain `name = value;` replaces the value of the loop. Add `Mesh.ElementOrder = 2;`, `Mesh.SaveGroupsOfNodes = 1;` and `Mesh.SaveGroupsOfElements = -100;` (2D) or `-1000;` (solid). Then `mesh.inp` has each physical group as a set, and no line elements. Use `SetFactory("OpenCASCADE");`: the built-in spline gives noise in the stress.
2. `job.inp`: `*INCLUDE, INPUT=mesh.inp`, the material, the section, 1 `*STATIC` step with the supports and the loads on the sets of `mesh.inp`, and `*EL FILE` with `S`. Do not add `OUTPUT=3D`. For plane stress, use a section thickness of at most 1/100 of the smallest radius. `ccx` expands each 2D element to a layer of this thickness. Multiply each force by the thickness, or give the load as a displacement.
3. `optimize.toml` has `field` (a field of `job.frd` with 1 value per node, for example `MISES`) and `set` (a node set of `mesh.inp`). It also has `[variables]`, with `name = { start = ..., low = ..., high = ... }` for each variable of `mesh.geo`.
4. Solve: `fh run CASE`. Each solve runs gmsh and `ccx`, and prints the variables and the objective: the largest value of the field on the set. The loop stops when the design changes by less than 0.001 of each range, after about 50 solves for 2 variables. Thus use a mesh that `ccx` solves in about 1 s. For the mesh study, run the loop with 2 element sizes.
5. Results: `optimize/optimum.txt` holds the best design and its objective, and `optimize/history.txt` holds each solve. The page shows the best design.

This loop agrees with the exact optimum of a hole in a plate under the remote stresses sx = 1 and sy = 0.5. The validation case is `shape-hole-biaxial`. It finds the ellipse with a/b = 2.002 and the peak 1.4992. The exact values are 2 and 1.5 (Cherepanov 1974).
