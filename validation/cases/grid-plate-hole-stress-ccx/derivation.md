# grid-plate-hole-stress-ccx derivation

This case compares the static stress solver on a mesh of 10-node tetrahedra with CalculiX on the same 3D problem: the steel plate with a round hole and the copper pad of `grid-plate-hole-ccx`, heated uniformly from 20 °C to 120 °C. The copper expands more than the steel, so the plate and the pad bend. The wall of the hole holds the 3 displacement components, for example a bolt. The problem has no analytical solution. The plate is a rectangle with a circle hole (a `polygon` with `holes`), and the pad is a box, so the solver meshes the model with tetrahedra (`fh guide grid`). Units are SI-m.

## Source

The reference is a CalculiX 2.23 solve with 10-node tetrahedra (C3D10) on a gmsh 4.15 mesh of the geometry of `grid-plate-hole-ccx`. `ccx/job.py` writes the input and `ccx/result.py` prints the values:

    gmsh ../grid-plate-hole-ccx/ccx/plate.geo -3 -setnumber H 0.001 -o mesh.inp
    python3 ccx/job.py mesh.inp job.inp
    ccx job
    python3 ccx/result.py job.dat

The 2 meshes are independent: gmsh meshes the true cylinder with curved second-order elements, and the built-in solver meshes a polygon with its points on the circle and straight edges.

## Model

| Item | Value |
|---|---|
| Plate | steel, E = 200 GPa, ν = 0.3, α = 12e-6 /K, x 0 to 0.1 m, y 0 to 0.06 m, z 0 to 0.01 m |
| Hole | radius 0.01 m, centre (0.03, 0.03), through the plate |
| Pad | copper, E = 110 GPa, ν = 0.34, α = 17e-6 /K, x 0.06 to 0.09 m, y 0.02 to 0.04 m, z 0.01 to 0.02 m |
| Temperature | 120 °C at each point, no strain at 20 °C |
| Wall of the hole | ux = uy = uz = 0 |

The 3 probes are the corners of `grid-plate-hole-ccx`, nodes of both meshes: `pad_corner` (0.09, 0.04, 0.02), `plate_far` (0.1, 0, 0) and `plate_near` (0, 0.06, 0.01).

## Mesh studies

Built-in solver, values in 1e-6 m:

| max_cell (m) | unknowns | pad ux | pad uy | pad uz | far ux | far uy | far uz | near ux | near uy |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0.004 | 88,887 | 79.358 | 17.561 | 16.810 | 77.450 | −35.999 | −18.559 | −33.042 | 32.497 |
| 0.003 | 180,975 | 79.285 | 17.556 | 16.872 | 77.465 | −36.013 | −18.327 | −33.040 | 32.491 |
| 0.002 | 438,414 | 79.241 | 17.573 | 16.887 | 77.474 | −36.023 | −18.184 | −33.037 | 32.488 |

CalculiX, C3D10, values in 1e-6 m:

| H (m) | nodes | pad ux | pad uy | pad uz | far ux | far uy | far uz | near ux | near uy |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0.004 | 9,874 | 79.375 | 17.523 | 16.796 | 77.447 | −35.999 | −18.752 | −33.060 | 32.532 |
| 0.002 | 61,045 | 79.258 | 17.569 | 16.873 | 77.469 | −36.022 | −18.232 | −33.036 | 32.489 |
| 0.001 | 424,717 | 79.183 | 17.564 | 16.937 | 77.480 | −36.034 | −17.984 | −33.029 | 32.479 |

## Tolerance

At the same cell size of 2 mm, the 2 solvers agree to 0.3 % on each value. The value that converges most slowly in the check is `pad_corner.uz`: the grid solver changes by 0.4 % from 4 mm to 2 mm, and it is within 0.5 % of its limit at 4 mm. CalculiX changes by 0.38 % from 2 mm to 1 mm, at about first order, so its reference is within 0.4 % of its limit. The sum is 0.9 %, and the tolerance is 1 %. The check uses max_cell 4 mm, so the test of each change solves it in about 2 s.

`plate_far.uz` is not in the check. It is the bending of the long free end of the plate, and the singular edges where the pad meets the plate and where the hole wall meets the free faces make it converge at first order in both solvers: CalculiX changes it by 2.8 % and then 1.4 %. The 2 solvers agree on it to 0.3 % at 2 mm.
