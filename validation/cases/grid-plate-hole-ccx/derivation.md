# grid-plate-hole-ccx derivation

This case compares the built-in solver on a mesh of tetrahedra with CalculiX on the same 3D steady problem: a steel plate with a round hole, and a copper pad with a heat source on the top of the plate. The wall of the hole is at 20 °C, for example a cooled bolt or a pipe. It has no analytical solution. The plate is a rectangle with a circle hole (a `polygon` with `holes`), and the pad is a box, so the solver meshes the model with tetrahedra (`fh guide grid`). Units are SI-m.

## Source

The reference is a CalculiX 2.23 solve with 10-node tetrahedra (C3D10) on a gmsh 4.15 mesh. `ccx/plate.geo` is the geometry and `ccx/job.py` writes the input:

    gmsh ccx/plate.geo -3 -setnumber H 0.001 -o mesh.inp
    python3 ccx/job.py mesh.inp job.inp
    ccx job

The 2 meshes are independent: gmsh meshes the true cylinder with curved second-order elements, and the built-in solver meshes a polygon with its points on the circle.

## Model

| Item | Value |
|---|---|
| Plate | steel, k = 50 W/(m K), x 0 to 0.1 m, y 0 to 0.06 m, z 0 to 0.01 m |
| Hole | radius 0.01 m, centre (0.03, 0.03), through the plate |
| Pad | copper, k = 400 W/(m K), x 0.06 to 0.09 m, y 0.02 to 0.04 m, z 0.01 to 0.02 m |
| Source | 2e6 W/m3 in the pad (12 W) |
| Wall of the hole | T = 20 °C |
| Other faces | adiabatic |

The 3 probes are corners of the geometry, so each is a node of both meshes: `pad_corner` (0.09, 0.04, 0.02), the top outer corner of the pad; `plate_far` (0.1, 0, 0), the corner of the plate far from the hole; `plate_near` (0, 0.06, 0.01), the corner of the plate near the hole.

## Mesh studies

Built-in solver, max_growth 1.3:

| max_cell (m) | points | pad_corner | plate_far | plate_near | Time |
|---:|---:|---:|---:|---:|---:|
| 0.004 | 3,350 | 35.35915 | 33.99452 | 20.91436 | 0.02 s |
| 0.002 | 20,024 | 35.60473 | 34.10883 | 20.90803 | 0.14 s |
| 0.001 | 144,672 | 35.77602 | 34.19500 | 20.90773 | 3.73 s |

CalculiX, C3D10:

| H (m) | nodes | pad_corner | plate_far | plate_near | Time |
|---:|---:|---:|---:|---:|---:|
| 0.004 | 9,874 | 35.70973 | 34.15845 | 20.90703 | 0.49 s |
| 0.002 | 61,045 | 35.82878 | 34.21981 | 20.90729 | 3.71 s |
| 0.001 | 424,717 | 35.88840 | 34.25052 | 20.90766 | 78.2 s |

## Tolerance

The edges where the pad meets the plate are re-entrant, so the heat flux is singular there, and the temperature far from them also converges slowly. CalculiX converges at about first order: each change is half of the last. Its first-order extrapolation is 35.947 °C at `pad_corner` and 34.281 °C at `plate_far`, so the error of the reference is at most 0.059 °C. The built-in solver converges more slowly (each change is about 0.7 of the last), as linear tetrahedra do near a singular edge. Against the extrapolated value, its error at max_cell 0.002 m is 0.342 °C at `pad_corner` and 0.172 °C at `plate_far`. The sum of the 2 errors is at most 0.40 °C. The tolerance is 0.5 °C.

The check uses max_cell 0.002 m, so the test of each change (a debug build) solves it in about 4 s. At max_cell 0.001 m, the error is 0.17 °C. `plate_near` is next to the cooled hole, and the 2 solvers agree there to 0.001 °C.
