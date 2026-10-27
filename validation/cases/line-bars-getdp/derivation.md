# line-bars-getdp derivation

This case compares the eddy currents of the transmission line solver (`fh guide grid`, a `[line]` table) with GetDP on the same 2D problem: 2 copper bars side by side, with the current out in 1 and back in the other, for example a bus bar pair. The skin effect and the proximity effect push the current to the faces of the bars that are near each other. It has no analytical solution. The case is not a chip package problem. Units are SI-m.

## Source

The reference is a GetDP 4.0 solve with linear nodal elements on a gmsh 4.15 mesh. `getdp/case.geo` is the geometry and `getdp/case.pro` the formulation: the vector potential a_z (`BF_PerpendicularEdge`), the voltage per length of each bar (`BF_RegionZ` with the global quantities U and I), the currents 1 A and −1 A, and a = 0 at 1 corner of the box:

    gmsh getdp/case.geo -2 -setnumber H 6.25e-6 -o case.msh
    getdp getdp/case.pro -msh case.msh -solve Eddy -pos Eddy

The loop impedance per length is Z = U2 − U1 (the files `u1.txt` and `u2.txt` give the real and the imaginary parts), and L = Im Z / ω. The 2 meshes are independent: gmsh grows the mesh from the bars with a distance field, and the built-in solver uses its own quality triangle mesh.

## Model

| Item | Value |
|---|---|
| Bar a | copper, σ = 5.8e7 S/m, x from −2.5 mm to −0.5 mm, y from −0.5 mm to 0.5 mm, the signal |
| Bar b | the same, x from 0.5 mm to 2.5 mm, the reference |
| Air | square, x and y from −20 mm to 20 mm |
| Walls | all magnetic (no electric wall), so the current of bar b is −1 A |
| Frequency | 100 kHz: the skin depth is 0.209 mm, and the bars are 4.8 skin depths thick |

## Mesh studies

GetDP, mesh size H at the bars, growing by 0.1 times the distance, up to 2 mm:

| H (m) | nodes | R (Ω/m) | L (H/m) |
|---:|---:|---:|---:|
| 50e-6 | 6,393 | 4.880894e-2 | 4.865151e-7 |
| 25e-6 | 12,034 | 4.870159e-2 | 4.868501e-7 |
| 12.5e-6 | 23,457 | 4.866722e-2 | 4.869753e-7 |
| 6.25e-6 | 46,801 | 4.865727e-2 | 4.870218e-7 |

Each change is about a third of the last, so the finest values are within 1e-4 of the converged R and L. They are the references.

Built-in solver (outline segments of a quarter of the skin depth, 52 µm):

| max_cell (m) | max_growth | points | R (Ω/m) | L (H/m) |
|---:|---:|---:|---:|---:|
| 2e-3 | 1.3 | 3,531 | 4.871861e-2 | 4.846324e-7 |
| 1e-3 | 1.3 | 5,861 | 4.875028e-2 | 4.849654e-7 |
| 5e-4 | 1.3 | 15,578 | 4.879902e-2 | 4.854998e-7 |
| 1e-3 | 1.1 | 10,889 | 4.878366e-2 | 4.867141e-7 |
| 5e-4 | 1.1 | 19,815 | 4.878652e-2 | 4.867439e-7 |
| 2.5e-4 | 1.1 | 57,675 | 4.879307e-2 | 4.868143e-7 |

L converges with the mesh of the air (`max_growth` 1.1), to 4.3e-4 below the reference. R converges to 2.8e-3 above the reference, the error of the outline segments: with segments of an eighth of the skin depth (a test build, max_cell 2.5e-4 m), R is 1.0e-3 above and L 3e-5 below the reference. So the 2 solvers converge to the same answer.

## Tolerance

The check uses max_cell 1e-3 m and max_growth 1.1: R is 2.7e-3 above and L 6.3e-4 below the reference. With the error of the reference (1e-4 for R and L), the largest sum is 2.9e-3. The tolerance is 5e-3 of each value.
