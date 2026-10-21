# topology-mbb-beam derivation

This case checks the topology loop of `fh run`: the minimum compliance of the half MBB beam with 50 % of the volume.
The reference is the result of a published SIMP code on the same mesh. The model uses the same method and an element with the same stiffness matrix. Thus the tolerance comes from the stop rule of the loop, not from a model error.

## Source

E. Andreassen, A. Clausen, M. Schevenels, B. S. Lazarov and O. Sigmund, "Efficient topology optimization in MATLAB using 88 lines of code", Structural and Multidisciplinary Optimization 43 (2011) 1-16, https://doi.org/10.1007/s00158-010-0594-7.

- The problem is the half MBB beam: a rectangle of 60 x 20 square elements of side 1. The left edge is a symmetry plane. A roller supports the bottom right corner. A load of 1 acts down at the top left corner.
- The material fills 50 % of the volume. The filter radius is 2.4. The penalty is 3.
- Figure 3, top left, gives c = 216.81 for the sensitivity filter. This is the check value.
- Figure 3 also gives c = 233.71 for the density filter, on the same problem. The case does not use this value.

## Model

The case `60x20` has the mesh and the loads of the paper. `mesh.py` writes its `job.inp`.

| Item | Value |
|---|---|
| Mesh | 60 x 20 elements `CPE4`, side 1, thickness 1 |
| Supports | ux = 0 on the left edge, uy = 0 at the bottom right corner |
| Load | `*CLOAD` of -1 in y at the top left corner |
| Loop | `volume = 0.5`, `radius = 2.4`, penalty 3, Emin = 1e-9 E0, move limit 0.2, stop when no density changes by more than 0.01 |

The paper uses the bilinear plane stress element with E0 = 1 and nu = 0.3. The `CPS4` element of `ccx` gives a stiffness that is 0.09 % higher: the compliance of the uniform design with x = 0.5 is 1006.132 with `CPS4` and 1007.022 with the element of the paper.
A plane strain element with E' = E0 (1 + 2 nu) / (1 + nu)^2 and nu' = nu / (1 + nu) has the same stiffness matrix as the plane stress element with E0 and nu. Thus the case uses `CPE4` with `young = 1.6 / 1.69` and `poisson = 0.3 / 1.3`.

## The loop

The loop of `fh` (`crates/fh/src/topology.rs`) follows the 88 line code with the sensitivity filter. In each iteration:

1. `fh` writes 1 material and 1 section for each element, with the modulus Emin + x^3 (E0 - Emin).
2. `ccx` solves the model and writes the strain energy `ELSE` and the volume `EVOL` of each element to `job.dat`.
3. The compliance is c = 2 times the sum of `ELSE`. The sensitivity of an element is -3 x^2 (E0 - Emin) u^T k0 u, with u^T k0 u = 2 `ELSE` / E of the element.
4. The sensitivity filter and the optimality criteria update give the new densities. The bisection finds the multiplier that keeps the volume at 50 %.

## Check of the loop

The case and 2 other loops ran on the same problem:

| Loop | Iterations | c |
|---|---:|---:|
| A Python copy of the 88 line method, with the element stiffness of the paper and a direct solver | 106 | 216.81366 |
| `fh run` with `CPE4` (the case) | 106 | 216.81366 |
| `fh run` with `CPS4`, E0 = 1 and nu = 0.3 | 103 | 216.5432 |

The case and the Python loop agree to all printed digits. Thus `ccx` and the reading of `job.dat` add no error.
`CPS4` gives a result 0.12 % below the reference, because its stiffness is 0.09 % higher.

## Tolerance

The tolerance is `rel_tol = 0.002` and `abs_tol = 0`.

The stop rule is at its limit in the last iterations. From iteration 95 to iteration 106, the largest change of a density is 0.0098 to 0.0123. The compliance oscillates in this band:

| Iterations | Smallest c | Largest c | Difference to 216.81 |
|---|---:|---:|---:|
| 95 to 106 | 216.668 | 217.064 | -0.07 % to +0.12 % |

A change in the last digit of a solve can stop the loop at a different iteration in this band. Thus the result can be 0.12 % from the reference with no error in the model.
`rel_tol` is 0.12 %, rounded up to 0.2 %. The case gives 216.8137, 0.002 % above the reference.
A change of the method gives a larger difference: with the density filter, the paper gives 233.71, 7.8 % above the check value.

## Run time

The loop solves the model 106 times. On a Mac with 10 cores, `fh validate` took 22 s for this case.
