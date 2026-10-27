# grid-heated-block-stress-ccx derivation

This case compares the grid solver with CalculiX on the same 3D problem of thermal stress: the steady temperature of a heated copper block on a steel plate, and the deformation that it causes (units SI-m). It checks the one-way coupling of the heat solve and the stress solve, a point support, and the shape of a surface. It has no analytical solution.

## Source

The reference is a CalculiX 2.23 solve with 20-node hexahedra (C3D20) and a coupled steady temperature and displacement step. `ccx/mesh.py` writes the input: `python3 ccx/mesh.py H job.inp` with the cell size H in x, y and z, then `ccx job`. `python3 ccx/result.py job.dat` prints the values of the checks. The CalculiX model is the same quarter as the grid model, with the same supports.

## Model

The heat problem is that of `grid-heated-block-ccx`.

| Item | Value |
|---|---|
| Plate | steel, x and y 0 to 0.1 m, z 0 to 0.01 m: k = 50 W/(m K), E = 200 GPa, ν = 0.3, α = 12e-6 1/K |
| Block | copper, x and y 0.035 to 0.065 m, z 0.01 to 0.03 m: k = 400 W/(m K), E = 110 GPa, ν = 0.34, α = 17e-6 1/K |
| Source | 2e6 W/m3 in the block |
| Bottom of the plate, z− | convection, h = 200 W/(m2 K), air at 20 °C |
| Stress-free temperature | 20 °C |
| Symmetry | x = 0.05 m and y = 0.05 m: the solvers solve the quarter x ≥ 0.05, y ≥ 0.05 |
| Support | uz = 0 at the point (0.05, 0.05, 0), the centre of the bottom |

The planes of symmetry hold ux and uy, and the point holds uz. Thus the supports stop only the rigid motions, and the plate is free to bend. The force of the point support is 0.

The checks are the displacements at the top of the block (0.05, 0.05, 0.03), at the bottom corner of the plate (0.1, 0.1, 0) and at the middle of the bottom edge (0.1, 0.05, 0), and the range of uz on the bottom face. The plate bends down at its edges: the bottom face is concave.

## Mesh studies

Grid solver, max_growth 1.3 (values in 1e-6 m):

| max_cell (m) | unknowns | block_top.uz | plate_corner.ux | plate_corner.uz | plate_edge.uz | Time |
|---:|---:|---:|---:|---:|---:|---:|
| 0.0025 | 7,468 | 12.6464 | 10.2806 | −7.3543 | −4.9290 | 0.12 s |
| 0.00125 | 52,344 | 12.7391 | 10.2845 | −7.2154 | −4.8369 | 1.08 s |
| 0.000625 | 390,256 | 12.7809 | 10.2864 | −7.1503 | −4.7926 | 13.8 s |

CalculiX, C3D20, quarter model (values in 1e-6 m):

| H (m) | nodes | block_top.uz | plate_corner.ux | plate_corner.uz | plate_edge.uz | Time |
|---:|---:|---:|---:|---:|---:|---:|
| 0.005 | 1,489 | 12.6707 | 10.2776 | −7.4151 | −4.9764 | 0.84 s |
| 0.0025 | 9,625 | 12.7480 | 10.2834 | −7.2445 | −4.8596 | 10.3 s |
| 0.00125 | 68,497 | 12.7844 | 10.2859 | −7.1640 | −4.8031 | 227 s, 5.5 GB |

`bottom.range` is equal to −`plate_corner.uz` in both solvers, because the centre is at 0 and the corner is the lowest point.

## Tolerance

The edge where the block meets the plate is singular in the stress, so both solvers converge at about first order: each change is about half of the last one (ratios 2.07 to 2.22). The first-order extrapolation of the 3 meshes gives:

| | grid solver | CalculiX |
|---|---:|---:|
| block_top.uz | 12.815 | 12.817 |
| plate_corner.uz | −7.093 | −7.092 |
| plate_edge.uz | −4.752 | −4.750 |

The 2 solvers agree to 0.03 % after the extrapolation. The error of the grid solver at max_cell 0.00125 m is at most 1.8 % (plate_edge.uz), and the error of the reference is at most 1.1 % (plate_edge.uz). Their sum is 2.9 %. The tolerance is 3 %.
