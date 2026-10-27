# grid-heated-block-ccx derivation

This case compares the grid solver with CalculiX on the same 3D steady problem: a copper block with a heat source on a steel plate that a convection cools (units SI-m). It has no analytical solution.

## Source

The reference is a CalculiX 2.23 solve with 20-node hexahedra (C3D20). `ccx/mesh.py` writes the input: `python3 ccx/mesh.py H job.inp` with the cell size H in x, y and z, then `ccx job`. The input has the full model, with no symmetry.

## Model

| Item | Value |
|---|---|
| Plate | steel, k = 50 W/(m K), x and y 0 to 0.1 m, z 0 to 0.01 m |
| Block | copper, k = 400 W/(m K), x and y 0.035 to 0.065 m, z 0.01 to 0.03 m |
| Source | 2e6 W/m3 in the block (36 W) |
| Bottom of the plate, z− | convection, h = 200 W/(m2 K), air at 20 °C |
| Other faces | adiabatic |
| Symmetry | x and y in the grid case: the solver solves 1 quarter |

The energy balance gives the mean bottom temperature: 20 + 36 / (200 · 0.01) = 38 °C.

## Mesh studies

Grid solver, max_growth 1.3:

| max_cell (m) | block_top | plate_corner | plate_edge_top |
|---:|---:|---:|---:|
| 0.005 | 47.7325 | 35.0277 | 36.9464 |
| 0.0025 | 48.1567 | 34.9996 | 36.9377 |
| 0.00125 | 48.3566 | 34.9880 | 36.9312 |
| 0.000625 | 48.4510 | 34.9829 | 36.9275 |

The solve at max_cell 0.000625 m has 131,537 unknowns and takes 1.15 s (6 PCG iterations).

CalculiX, C3D20:

| H (m) | block_top | plate_corner | plate_edge_top | Time |
|---:|---:|---:|---:|---:|
| 0.005 | 48.19073 | 34.99600 | 36.93994 | 0.53 s |
| 0.0025 | 48.37077 | 34.98685 | 36.93147 | 5.27 s |
| 0.00125 | 48.45740 | 34.98249 | 36.92743 | 131.7 s |

The finest CalculiX mesh has 267,377 nodes.

## Tolerance

The edge where the block meets the plate is re-entrant, so the heat flux is singular there. Thus `block_top` converges at about first order in both solvers: each change is about half of the last one. The first-order extrapolation gives 48.54 °C from the grid solver and 48.54 °C from CalculiX. The error of the grid solver at max_cell 0.000625 m is about 0.084 °C, and the error of the reference is about 0.080 °C. Their sum is 0.164 °C. The tolerance is 0.2 °C.

`plate_corner` and `plate_edge_top` are far from the singular edge. There, the 2 solvers agree to 0.0004 °C, but the check file has 1 tolerance for all checks, so they also have 0.2 °C.
