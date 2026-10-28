# shape-hole-biaxial derivation

## Conclusion

The shape loop of `fh run` finds the exact optimum hole, the ellipse with the axis ratio a/b = 2. The loop makes the peak stress a minimum. The peak stress is the largest von Mises stress on the hole edge. After 52 solves, the ratio is 2.00229 (+0.11 %), the power of the superellipse is 2.00061 (+0.03 %), and the peak stress is 1.49918 (-0.05 %). The tolerance is 0.5 %.

The loop starts at the rounded square with ratio 1 and power 3, with a peak of 2.39. The ratio and the power go to 2, and the peak goes to 1.5, the lowest peak of all hole shapes:

**The loop: each solve of the hole shape**

![The loop: each solve of the hole shape](shape-hole-biaxial/loop.svg)

peak is the largest von Mises stress on the hole edge. The exact optimum is the ellipse with ratio 2 and power 2, and its peak 1.5 is the lowest peak of all hole shapes (derivation.md).

The best hole has almost the same stress at each point of its edge. This agrees with Cherepanov (1974), who shows that the optimum hole has a constant hoop stress on its edge. At the nodes of the edge, the von Mises stress is between 1.4973 and 1.4992. The plate of half side 40 and the mesh make it a little lower than 1.5 (see "Tolerance").

**The stress on the edge of the best hole**

![The stress on the edge of the best hole](shape-hole-biaxial/edge.svg)

On the free edge, the von Mises stress is equal to the absolute value of the hoop stress. The model is between 1.4973 and 1.4992 at the nodes of the edge. The exact ellipse has the hoop stress sx + sy = 1.5 at each point of its edge (derivation.md).

The 3D view shows the von Mises stress near the best hole, from the top. The colour on the edge is almost the same from the x axis to the y axis. The dots mark the peak, 1.4992 at 8.4 degrees from the x axis, and the end of the y axis, 1.4977.

## Result

**Pass.** A check passes when |computed - reference| ≤ rel_tol · |reference| + abs_tol, with rel_tol = 0.005 and abs_tol = 0.0 (`check.toml`). `fh validate` gives the computed values.

| Check | Unit | Computed | Reference | Difference | Tolerance | Margin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `ratio` | Not specified | 2.00229 | 2 | +0.0022905 (+0.114525 %) | 0.01 | 0.007709 |
| `power` | Not specified | 2.00061 | 2 | +0.000605917 (+0.0302959 %) | 0.01 | 0.009394 |
| `objective` | Not specified | 1.49918 | 1.5 | -0.000816864 (-0.0544576 %) | 0.0075 | 0.006683 |

Made by `scripts/validation-reports.py` on 2026-10-28, femahoy 5ac0a46.

This case checks the shape optimization loop of `fh run`: the hole of minimum stress concentration in a large plate under the biaxial remote stress sx = 1, sy = 0.5.
The exact optimum is known: the ellipse with the axis ratio a/b = sx/sy = 2. Its hoop stress is sx + sy = 1.5 at each point of the edge.
The loop changes 2 variables of a superellipse hole and finds the optimum in 52 solves.

## Source

- G. P. Cherepanov, "Inverse problems of the plane theory of elasticity", Journal of Applied Mathematics and Mechanics 38 (1974) 915-931. When the 2 remote stresses have the same sign, the hole of minimum stress concentration has a constant hoop stress on its edge (the equal-strength hole). For 1 hole, this hole is an ellipse with a/b = sx/sy.
- C. E. Inglis, "Stresses in a plate due to the presence of cracks and sharp corners", Transactions of the Institution of Naval Architects 55 (1913) 219-241. The hoop stress on the edge of an elliptical hole. At the end of the x axis (a, 0), it is sy (1 + 2 a/b) - sx. At the end of the y axis (0, b), it is sx (1 + 2 b/a) - sy.
- K. Bandara, T. Rüberg and F. Cirak, "Shape optimisation with multiresolution subdivision surfaces and immersed finite elements", Computer Methods in Applied Mechanics and Engineering 300 (2016) 510-539, https://doi.org/10.1016/j.cma.2015.11.015, section 5.1.2. The same problem is a benchmark of shape optimization. For the stress ratio 0.5, the aspect ratio of the result goes to the exact value when the plate becomes large.

No hole shape gives a peak below sx + sy. The sum sx + sy of the 2 normal stresses is a harmonic function in the plate. It goes to the remote value 1.5 far from the hole. On the free edge, it is equal to the hoop stress. Thus the hoop stress cannot be below 1.5 at all points of the edge, and its largest value is at least 1.5.

With sx = 1 and sy = 0.5:

| Hole | Peak of the hoop stress | Point |
|---|---:|---|
| Circle | 3 sx - sy = 2.5 | (0, R) |
| Ellipse with a/b = k, k < 2 | 0.5 + 2/k | (0, b) |
| Ellipse with a/b = k, k > 2 | k - 0.5 | (a, 0) |
| Ellipse with a/b = 2 | 1.5 | each point |

## Model

The case `L40` is a quarter of the plate. `mesh.geo` makes the mesh with gmsh, and `job.inp` is the CalculiX model.

| Item | Value |
|---|---|
| Hole | the superellipse \|x/a\|^power + \|y/b\|^power = 1 with a/b = ratio and a b = 1, a spline through 61 points |
| Plate | the square 0 <= x, y <= 40 (half side 40 = 40 times the radius of the circle with the same a b) |
| Mesh | 6-node plane stress triangles (`CPS6`), element size 0.025 + 0.15 d at the distance d from the hole, 2056 elements for the exact ellipse |
| Material | E = 1000, nu = 0.3, thickness 0.01 |
| Supports | ux = 0 on x = 0, uy = 0 on y = 0 (symmetry) |
| Load | the remote strain on the far edges: ux = 40 (sx - nu sy) / E = 0.034 on x = 40, uy = 40 (sy - nu sx) / E = 0.008 on y = 40 |
| Objective | the largest von Mises stress `MISES` at a node of the hole edge (node set `HOLE`) |

On the free edge, the von Mises stress is equal to the absolute value of the hoop stress.
`ccx` expands each plane stress element to a layer with the section thickness. A thick layer gets a stress SZZ that is not 0, and the circle then gives a peak 1.2 % too low. Thus the thickness is 0.01.
The far edges move. They do not get a pressure, because the gmsh `.inp` file has no element faces for `*DLOAD`. The 2 boundary conditions give the same stress far from the hole. The study of the plate size below shows that the difference near the hole decreases as 1/L^2, with L the half side of the plate.

## The loop

The loop of `fh` (`crates/fh/src/optimize.rs`) is the Nelder-Mead method on the variables `ratio` (1 to 4) and `power` (1.6 to 4), each scaled to 0 to 1. For each design:

1. gmsh meshes `mesh.geo` with `-setnumber ratio ... -setnumber power ...`.
2. `ccx` solves `job.inp`.
3. The objective is the largest `MISES` of the last frame of `job.frd` at the nodes of the set `HOLE`.

The first simplex has steps of 0.1 of each range. The loop stops when each point of the simplex is in 0.001 of the range from the best point. The start is the rounded square with ratio 1 and power 3 (peak 2.39).

## Check of the model

The circle and the exact ellipse (ratio 2, power 2), each in 1 solve:

| Half side of the plate | Elements of the ellipse | Circle | Ellipse | Error of the ellipse |
|---:|---:|---:|---:|---:|
| 10 | 1764 | 2.45005 | 1.48895 | -0.74 % |
| 20 | 1922 | 2.48672 | 1.49724 | -0.18 % |
| 40 (the case) | 2056 | 2.49607 | 1.49929 | -0.05 % |
| 80 | 2197 | 2.49846 | 1.49981 | -0.01 % |

Each time the plate size doubles, the error decreases by a factor of about 4. The circle and the ellipse go to the exact values 2.5 and 1.5.

The mesh at the half side 40:

| Element size at the edge | Elements of the ellipse | Circle | Ellipse |
|---:|---:|---:|---:|
| 0.05 | 1341 | 2.49450 | 1.49929 |
| 0.025 (the case) | 2056 | 2.49607 | 1.49929 |
| 0.0125 | 3292 | 2.49652 | 1.49930 |
| 0.00625 | 5731 | 2.49658 | 1.49931 |

At the optimum, the mesh changes the peak by less than 0.01 %.

Near the optimum, the peak is the larger value of the 2 axis ends. Thus the objective is not smooth at the optimum:

| Ratio | Power | Peak | Inglis |
|---:|---:|---:|---:|
| 1.98 | 2 | 1.50937 | 1.51010 |
| 2 | 2 | 1.49929 | 1.5 |
| 2.02 | 2 | 1.51646 | 1.52 |
| 2 | 1.95 | 1.59378 | |
| 2 | 2.05 | 1.52832 | |

The peak increases by 0.5 to 0.86 per unit of ratio and by 0.58 to 1.9 per unit of power.

## Check of the loop

| Start (ratio, power) | Solves | Ratio | Power | Peak |
|---|---:|---:|---:|---:|
| 1, 3 (the case) | 52 | 2.00229 | 2.00061 | 1.49918 |
| 3, 2.5 | 52 | 2.00024 | 2.00000 | 1.49917 |

A Python copy of the loop with the same steps gave the same values to all digits.
The 2 starts have a ratio below and above the optimum. Both find the ellipse with a/b = 2.

## Tolerance

The tolerance is `rel_tol = 0.005` and `abs_tol = 0`.

| Error | Ratio | Power | Peak |
|---|---:|---:|---:|
| Stop rule: 0.001 of the range 3 of ratio and 2.4 of power | 0.15 % | 0.12 % | at most +0.5 % |
| Plate of half side 40 | about 0.1 % | about 0.1 % | -0.05 % |
| Mesh | below 0.01 % | below 0.01 % | below 0.01 % |
| Sum | at most 0.25 % | at most 0.25 % | -0.06 % to +0.5 % |

- The stop rule: at the stop, the best point is about 0.003 in ratio and 0.0024 in power from the optimum. With the slopes of the table above (at most 1 and 1.9), the peak is at most 0.0076 above the minimum, 0.5 %.
- The plate: the finite plate changes the 2 axis ends by different values. At the half side 40, the end (a, 0) is about 0.2 % low and the end (0, b) is about 0.05 % low. The 2 ends are equal at a ratio about 0.1 % above 2.
- The check value of the peak is the exact 1.5. The model gives 1.49929 for the exact ellipse, 0.05 % below. A peak below 1.5 comes from the plate and the mesh only.

The case gives ratio 2.00229 (0.11 %), power 2.00061 (0.03 %) and peak 1.49918 (-0.05 %).
A wrong method gives a larger difference: the circle gives 2.5, 67 % above the check value, and the start design gives 2.39.

## Run time

The loop solves the model 53 times: 52 designs and 1 more solve of the best design with the 3D output for the page. Each solve takes about 1.5 s. On a Mac with 10 cores and a high load from other programs, `fh validate` took 79 s for this case.
