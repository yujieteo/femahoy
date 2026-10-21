# team7-eddy-current derivation

This case checks the electromagnetic path: 3D eddy currents at 50 Hz in an aluminium plate with a hole, under a coil.
The reference is a measurement with no stated uncertainty. Thus the tolerance comes from the agreement of an independent code with the same data and from the mesh study of the model.

## Why this case

No other case of the suite has any of these items:

- Electromagnetics: the solver is GetDP, not CalculiX or OpenFOAM.
- Complex fields of a time-harmonic problem. The check reads 2 phases of the same field.
- Edge elements (H(curl)) for the vector potential, with a gauge.
- An unbounded region: a shell of elements maps the air to infinity.
- A conductor with a hole, thus a multiply connected conductor.
- The skin effect: the skin depth at 50 Hz is sqrt(2 / (omega mu0 sigma)) = 12.0 mm, and the plate is 19 mm thick.

## Sources

- K. Fujiwara and T. Nakata, "Results for benchmark problem 7 (asymmetrical conductor with a hole)", COMPEL 9 (1990) 137-154, https://doi.org/10.1108/eb010071.
  It defines TEAM problem 7, gives the measured values (Bz in its Table 4), and compares 25 solutions of 17 codes with them.
- The table of the measured Bz on the line A1-B1 is also on 2 web pages:
  - the TEAM problems of NGSolve, https://ngsolve.github.io/TEAM-problems/TEAM-7/team7.html;
  - the TEAM 7 case of mufem, https://github.com/Raiden-Numerics/mufem/blob/main/Electromagnetics/Compumag-Team7-Asymmetrical-Conductor-with-a-Hole/README.md.

  The 2 copies have the same values at the points of the check. They differ at x = 18 mm, 90 degrees (2.84 G and 2.48 G). The check does not use that point.
- The mufem page gives the relative L2 difference between its solution and these data, over the 17 points and the 2 phases of A1-B1: 3.3 % at 50 Hz and 5.9 % at 200 Hz.

The line A1-B1 is y = 72 mm, z = 34 mm, from x = 0 to x = 288 mm. The plate is from z = 0 to z = 19 mm, thus the line is 15 mm above the plate.
The source gives Bz at the instants omega t = 0 and omega t = 90 degrees, with the coil current at its peak at omega t = 0.

| Check | x (mm) | omega t | Measured Bz (G) | Value (T) | Why this point |
|---|---:|---:|---:|---:|---|
| `bz0_36` | 36 | 0 | -22.13 | -2.213e-3 | the largest \|Bz\| above the hole |
| `bz90_36` | 36 | 90 | 4.15 | 4.15e-4 | the same point |
| `bz0_126` | 126 | 0 | 78.11 | 7.811e-3 | the largest Bz at 0 degrees |
| `bz90_126` | 126 | 90 | 4.97 | 4.97e-4 | the same point |
| `bz0_162` | 162 | 0 | 60.44 | 6.044e-3 | the same point as below |
| `bz90_162` | 162 | 90 | 14.15 | 1.415e-3 | the largest Bz at 90 degrees |

## Model

The GetDP case is the folder `50hz`: `case.geo` for gmsh and `case.pro` for GetDP. The units are SI, with lengths in m.

| Item | Value |
|---|---|
| Plate | 294 x 294 x 19 mm, from (0, 0, 0). The hole is 108 x 108 mm, from x, y = 18 mm to 126 mm. sigma = 3.526e7 S/m |
| Coil | 2742 ampere-turns, a uniform current density in the section of 25 x 100 mm. The inner edge is a square of 150 mm with corner radius 25 mm, the outer edge a square of 200 mm with corner radius 50 mm. Centre (194, 100) mm, from z = 49 mm to 149 mm. The current goes anticlockwise as seen from +z |
| Air | a sphere of radius 0.35 m about (147, 147, 74) mm. Around it, a shell to 0.45 m that GetDP maps to infinity (`VolSphShell`). a = 0 on the outer face |
| Formulation | the modified vector potential a, with E = -j omega a in the plate: curl(nu curl a) + j omega sigma a = js. First-order edge elements |
| Gauge | a tree of edges in the coil and the air (`EdgesOfTreeIn`, `StartingOn` the outer face and the skin of the plate). The edges of the skin have a separate coefficient that the gauge does not fix |
| Bz at a point | the L2 projection of curl a on first-order node functions in a band about A1-B1 (x = -6 to 294 mm, y = 62 to 82 mm, z = 26 to 42 mm) |
| Phase | GetDP uses the time factor exp(j omega t). Thus Bz at omega t = 0 is Re(Bz), and at 90 degrees it is -Im(Bz) |
| Mesh | first-order tetrahedra. Size 5 mm in the plate, 10 mm in the coil, 3 mm in the band and 50 mm at the sphere. 43848 nodes, 278808 unknowns |
| Output | `bz.txt`: 1 row with the 6 values of the check. `j.pos`: the eddy current density in the plate, for the viewer |

Problems that the model solves:

- A tree gauge that starts on the skin of the plate also fixes the edges of the tree on the skin. In the first model, the skin edges had no separate coefficient. Then the 0 degree values were about 15 % to 20 % below the data, and the 90 degree values were about 4 times too small.
- In a first-order edge element, curl a is constant. Thus the value at a point changes much from element to element. The projection in the band gives a continuous field.
- A box of air with a = 0 on its faces changes the result with the size of the box. The shell that maps to infinity removes this error.

## Check of the solve

Without the plate, there are no eddy currents. Then the model must give the field of the coil alone. A Biot-Savart sum over the coil section gives Bz = 88.3 G at x = 126 mm and 103.9 G at x = 162 mm. The GetDP model without the plate agreed with the sum to 1 % to 2 % on A1-B1.
Without eddy currents, the checks fail: `bz0_126` would be 88.3 G against 78.11 G, and each 90 degree value would be 0.

## Mesh study

These commands, in the repository root, solve the case on another mesh in a scratch folder. `-setnumber` sets the mesh sizes of `case.geo` in m:

```
mkdir /tmp/team7 && cp validation/cases/team7-eddy-current/50hz/case.* /tmp/team7 && cd /tmp/team7
gmsh case.geo -3 -o case.msh -setnumber hp 0.0035 -setnumber hc 0.007 -setnumber hb 0.0021 -setnumber hs 0.035
getdp case.pro -msh case.msh -solve '#1' -pos '#1'
```

The values are in `/tmp/team7/bz.txt`. The 3 meshes use the sizes of the case times 1.5, 1 and 0.7.

| Mesh | Unknowns | Time | bz0_36 | bz90_36 | bz0_126 | bz90_126 | bz0_162 | bz90_162 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| x 1.5 | 98814 | 5 s | -23.25 | 4.88 | 76.97 | 4.91 | 58.18 | 15.32 |
| x 1, the check | 278808 | 23 s | -23.14 | 4.81 | 78.06 | 4.84 | 58.82 | 14.79 |
| x 0.7 | 735507 | 192 s | -23.11 | 4.76 | 78.72 | 4.83 | 58.61 | 14.68 |
| Measured | | | -22.13 | 4.15 | 78.11 | 4.97 | 60.44 | 14.15 |

The values are in G (1 G = 1e-4 T). The times are the times of GetDP on a Mac with 10 cores.

The largest change from the check mesh to the fine mesh is 0.66 G (0.84 %), in `bz0_126`. There the values increase monotonically. The observed order is 1.33, and the extrapolated value is 79.81 G. Thus the mesh error of the check mesh is 1.75 G, 2.2 %.
At the other points, the values do not change monotonically. From the check mesh to the fine mesh, the 0 degree values change by 0.36 % or less, and the 90 degree values by 0.11 G or less.

## Tolerance

The tolerance is `rel_tol = 0.06` and `abs_tol = 1e-4` (T).

| Cause | Basis | Effect |
|---|---|---:|
| The measurement and the model | The source gives no uncertainty. The solution of mufem has a relative L2 difference of 3.3 % from these data on A1-B1 at 50 Hz. | 3.3 % |
| Mesh error of the check mesh | The extrapolation of `bz0_126` from 3 meshes | 2.2 % |

`rel_tol` is the sum, 5.5 %, rounded up to 6 %.

The 90 degree values are small, 4 to 15 G. A small error in the phase of the measurement moves them by a part of the 0 degree value: 1 degree of phase moves `bz90_126` by 78.11 G x sin(1 degree) = 1.4 G. The source gives no uncertainty of the phase. `abs_tol` = 1 G is 0.7 degree of phase at x = 126 mm.

| Check | Model (G) | Measured (G) | Difference (G) | Tolerance (G) | Part of the tolerance |
|---|---:|---:|---:|---:|---:|
| `bz0_36` | -23.14 | -22.13 | -1.01 | 2.33 | 44 % |
| `bz90_36` | 4.81 | 4.15 | 0.66 | 1.25 | 53 % |
| `bz0_126` | 78.06 | 78.11 | -0.05 | 5.69 | 1 % |
| `bz90_126` | 4.84 | 4.97 | -0.13 | 1.30 | 10 % |
| `bz0_162` | 58.82 | 60.44 | -1.62 | 4.63 | 35 % |
| `bz90_162` | 14.79 | 14.15 | 0.64 | 1.85 | 35 % |

The tolerance stops these errors:

- No eddy currents (the coil alone): `bz0_126` is 10.2 G away, and each 90 degree value is 4.15 G or more away.
- The gauge that also fixes the skin edges: the 0 degree values are 15 % to 20 % low, and the 90 degree values are about 4 times too small.

The coarse mesh (x 1.5) passes. Its largest part of the tolerance is 63 % (`bz90_162`).

## Not in the check

- 200 Hz: the 90 degree values are 1 to 5 G, near `abs_tol`, and mufem has 5.9 % at 200 Hz. A second frequency also doubles the time of the case.
- The eddy current density in the plate and the line A2-B2: the source also gives them. Bz on A1-B1 is the quantity that most solutions of this problem compare.

On a Mac with 10 cores, `fh run` of the case took 25 s with a peak memory of 6.9 GB. gmsh took 2 s of it.
