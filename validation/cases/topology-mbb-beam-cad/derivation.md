# topology-mbb-beam-cad derivation

This case checks the CAD of a topology run: the solid that `fh run` makes from the last design, and the check of this solid with `ccx`.
The case 60x20 is the same as in `topology-mbb-beam`: the half MBB beam with 50 % of the volume. The check compares the compliance of the solid with the compliance of the loop.
The solid has no gray material. Thus its compliance is lower than the compliance of the loop, and the tolerance comes from this difference, not from a model error.

## Source

E. Andreassen, A. Clausen, M. Schevenels, B. S. Lazarov and O. Sigmund, "Efficient topology optimization in MATLAB using 88 lines of code", Structural and Multidisciplinary Optimization 43 (2011) 1-16, https://doi.org/10.1007/s00158-010-0594-7.

Figure 3, top left, gives c = 216.81 for the sensitivity filter. This is the check value. The loop of `fh` gives 216.8137 on this problem (`topology-mbb-beam/derivation.md`).

## Model

`60x20/job.inp` and `60x20/topology.toml` are copies of the files of `topology-mbb-beam`. The `mesh.py` of that case writes `job.inp`.

After the loop, `fh run` writes the CAD in `topology/cad` (`crates/fh/src/topology/cad.rs`):

1. The density of a node is the mean of the densities of its elements. The material is where the density is 0.5 or more.
2. The boundary of the material is a set of closed loops. On the edge of the mesh, a loop has straight lines. In the mesh, a loop is a smoothed `Spline` of gmsh.
3. `design.geo` holds the plane surfaces of the material and their extrusion by the thickness 1. gmsh writes the solid to `design.step`.
4. gmsh meshes the plane surfaces with `CPE6` elements of size 0.5, half the element size of the loop. The material has the modulus E0 of the loop (the density 1). The nodes on the left edge get ux = 0, the node at the bottom right corner gets uy = 0, and the node at the top left corner gets the load. `ccx` solves this model, and `cad/check.txt` holds the compliance c = 2 times the sum of `ELSE`.

## Results

| Model | Volume fraction | c | Difference to 216.81 |
|---|---:|---:|---:|
| The loop (`topology-mbb-beam`) | 0.500 | 216.81 | 0.00 % |
| The threshold at 0.5 on the 1200 elements of the loop: 588 elements with E0, the others void | 0.490 | 193.27 | -10.86 % |
| The CAD, `CPE6` of size 0.5 (the case) | 0.510 | 190.48 | -12.15 % |

The loop gives an element with the density x the stiffness x^3 E0. In the last design, 588 elements have a density of 0.5 or more, and many of them are gray (below 1). The solid gives these elements the full stiffness E0. Thus the solid is 10.86 % stiffer than the last design.
The CAD is a further 1.45 % stiffer than the threshold on the elements. Its smooth boundary follows the 0.5 level of the density. The solid holds 0.510 of the volume, and the 588 elements hold 0.490.

## Mesh of the check

The load is a point load. In plane elasticity, the displacement under a point load is infinite. Thus c increases each time the mesh becomes finer, and it has no limit:

| Size of the `CPE6` elements | Elements | c |
|---:|---:|---:|
| 1 | 1551 | 188.35 |
| 0.5 (the case) | 6031 | 190.48 |
| 0.25 | 23341 | 192.85 |
| 0.125 | 91879 | 195.25 |

Each halving of the size adds 1.1 % to 1.3 % to c. The loop has the same effect on its mesh of size 1. Thus the check uses a fixed size, half the element size of the loop, and the tolerance does not include a mesh error.
At the size 0.5, the 3 mesh algorithms of gmsh for triangles (`meshadapt`, `del2d`, `front2d`) give c = 190.27 to 190.48. Thus a change of the mesh at the same size changes c by 0.11 %.

## Tolerance

The tolerance is `rel_tol = 0.15` and `abs_tol = 0`.

| Item | Difference |
|---|---:|
| The threshold at 0.5 (no gray material) | -10.86 % |
| The smooth boundary and the mesh of the check | -1.45 % |
| A different mesh of the same size (3 algorithms of gmsh) | 0.11 % |
| The stop rule of the loop (`topology-mbb-beam`) | 0.12 % |
| Sum of the magnitudes | 12.54 % |

`rel_tol` is 12.54 %, rounded up to 15 % for other versions of gmsh and `ccx`. The case gives 190.48, 12.15 % below the reference.
The tolerance does not hide a large error of the CAD. For example, a part that the supports do not hold stops the check, and a thickness of 2 instead of 1 halves c (-56 %).

## Run time

The loop solves the model 106 times, then gmsh and `ccx` make and solve the CAD. On a Mac with 10 cores, `fh validate` took 17 s for this case.
