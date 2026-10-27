# grid-radiating-fin-ccx derivation

This case compares the radiation and the table conductivity of the built-in grid solver with CalculiX on the same mesh: a steel pin fin in vacuum. The base is hot, and all other faces radiate to cold surroundings. It has no analytical solution. The case is not a chip package problem. Units are SI-m, and the temperatures are in kelvin.

## Source

The reference is a CalculiX 2.23 steady heat transfer solve with 8-node hexahedra (DC3D8) on the same uniform grid as `fin/grid.toml`, with `*RADIATE` faces and the same conductivity table. `ccx/mesh.py` writes the input: `python3 ccx/mesh.py H job.inp` with the cell size H, then `ccx job`.

## Model

| Item | Value |
|---|---|
| Fin | x and y 0 to 0.004 m, z 0 to 0.05 m |
| Conductivity | the table (300 K, 14), (500 K, 17.5), (700 K, 20) W/(m K), linear between the rows |
| Base, z = 0 | 600 K |
| Sides and tip | radiation, eps = 0.9, T_amb = 300 K, sigma = 5.670374419e-8 W/(m2 K4) |
| Probes | `tip` at the centre of the tip, `middle` on the axis at z = 0.025 m |

## Mesh studies

| H, max_cell (m) | grid `tip` (K) | CalculiX `tip` (K) | grid `middle` (K) | CalculiX `middle` (K) | grid base heat (W) | CalculiX base heat (W) |
|---:|---:|---:|---:|---:|---:|---:|
| 0.001 | 446.06933 | 446.0565 | 480.32202 | 480.3071 | 2.24864 | 2.199516 |
| 0.0005 | 446.05898 | 446.0581 | 480.30948 | 480.3057 | 2.24812 | 2.223454 |
| 0.00025 | 446.05635 | 446.0584 | 480.30629 | 480.3052 | 2.24799 | 2.235632 |

The grid solver converges at second order. Its Newton iteration took 8 iterations on each grid. The 2 solvers have different quadratures on the same mesh: the grid solver takes the conductivity of a cell at its mean temperature and lumps the radiation at the points of a face, and CalculiX integrates both at the Gauss points. Thus on the same mesh they differ by an error of second order in the cell size. The base heat of CalculiX (the total `RFL` of the base points) converges at first order to about 2.2478 W, the limit of the grid solver. The check file has 1 tolerance for all checks, so it has no check of the base heat.

## Tolerance

The checks are the CalculiX values at H = 0.001 m, the mesh of `fin/grid.toml`. On this mesh, the largest difference is 3.1e-5 of the value (`middle`, 0.0149 K). The tolerance 1e-4 holds it with a margin of a factor of 3. The budget of the method was 1 % against CalculiX on the same mesh.
