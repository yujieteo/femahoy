# Steel plate on its edge, through CalculiX

This case is the plate model from a chat with the owner.
The first 2 chat solves failed with the own solver, thus this case runs the model through `ccx`.
The reference is the converged value of a separate graded hex model in CalculiX.
The 20 mm tetrahedral mesh of the chat is 3.8 percent from it.

## Model

Axes: x along the length (origin at the middle), y up, z through the thickness.
The plate is 3756 x 1000 x 2.4 mm, steel with E = 210000 MPa and nu = 0.3.

| Item | Value |
|---|---|
| Fixed ends | x = -1878 and x = +1878: all faces of the end, ux = uy = uz = 0 |
| Pins | 20 mm pads on the bottom edge (y = -500), at x = -1300 and x = +1300: uy = uz = 0 |
| Upward forces | 2700 N on a 100 mm pad of the top edge at x = -1500 and at x = -1200 |
| Downward forces | 2700 N with a moment of +800000 Nmm about z, on a 100 mm pad of the top edge at x = +1200 and at x = +1500 |

Each load is a pressure on the 2 halves of its pad (50 x 2.4 mm each):

- Upward 2700 N: p = -2700 / (100 x 2.4) = -11.25 MPa on the 2 halves.
- Downward 2700 N with +800000 Nmm: the half centroids are 25 mm from the load point.
  Thus F_left + F_right = -2700 N and 25 (F_right - F_left) = 800000 Nmm.
  F_left = -17350 N gives p = 144.583333 MPa. F_right = +14650 N gives p = -122.083333 MPa.

The sum of the applied forces is 0 N.
The moment sign is an assumption of the chat: +z by the right-hand rule.

The chat added uz = 0 on the back face (z = -1.2) in its third solve, because the own solver did not converge without it.
The owner did not ask for this support, thus the case does not have it.
The fixed ends hold all rigid motions of the plate, so the model needs no other support.

## Mesh of the case

The STEP file comes from `part.py` (build123d through `fh-python`).
The input `part.inp` comes from these commands in this folder:

```
fh-python part.py
fh mesh part.step --case . --size 20
fh model part.msh --case .
```

The result is the input of the second chat solve, without change.
It has 139395 nodes and 69045 C3D10 elements, with 1 element through the thickness.

## Reference

The beam theory of the chat is not a valid reference.
It gives 4998 N at the right pin and -4048 N at the left pin.
The converged value of the finite element models is 2279 N and -1275 N.

The cause is the pin pads. Beam theory puts each support on the full section.
In the plate, each pin is a 20 mm pad on the bottom edge, 500 mm below the axis.
The plate deforms locally near each pad, thus a pin is less stiff than a support on the full section.
Then the pins take less load and the fixed ends take more load.
A check model holds uy on a 20 mm strip through the full depth at each pin.
It gives 5939 N and -4023 N, near the beam values.

The reference comes from `reference.py`. It writes a CalculiX input of the same plate:

- C3D8I hexahedral elements, 1 layer through the thickness.
- The element size is 1 mm at each pad edge, load point and plate corner. It increases by a factor of 1.3 to 40 mm.
- Each level halves each element in x and in y. Thus the mesh ratio is 2.

Run `python3 reference.py LEVEL > plate.inp`, then `ccx -i plate`. The reactions are the `TOTALS` lines of `plate.dat`.

| Level | Elements | Nodes | Smallest element (mm) | Left pin (N) | Right pin (N) | Time (s) | Memory (GiB) |
|---:|---|---:|---:|---:|---:|---:|---:|
| 0 | 326 x 43 x 1 | 28776 | 1.14 | -1282.97 | 2293.24 | 1.4 | 0.69 |
| 1 | 652 x 86 x 1 | 113622 | 0.571 | -1278.48 | 2285.60 | 6.2 | 2.65 |
| 2 | 1304 x 172 x 1 | 451530 | 0.285 | -1276.41 | 2282.13 | 35.0 | 6.79 |

Richardson extrapolation over the 3 levels gives these values:

| Quantity | Observed order | Limit (N) | Error of level 2 |
|---|---:|---:|---:|
| Left pin | 1.11 | -1274.62 | 0.14 % |
| Right pin | 1.14 | 2279.23 | 0.13 % |

The observed order is near 1, because the pad edges and the corners of the fixed ends are singular points.
Level 1 with 2 layers through the thickness changes the pin reactions by 0.09 % or less.
Thus the error of the reference is 0.2 % or less.
The sum of the 4 reactions is 0 N at each level.

## Tolerance

The relative tolerance is 0.05.
The tetrahedral meshes approach the reference as the mesh becomes finer:

| Size (mm) | Nodes | Elements | Left pin (N) | Right pin (N) | Left error | Right error | Time (s) | Memory (GiB) |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 40 | 41400 | 20541 | -1342.13 | 2387.40 | 5.30 % | 4.75 % | 3.0 | 0.77 |
| 20 | 139395 | 69045 | -1322.91 | 2355.62 | 3.79 % | 3.35 % | 12.0 | 2.91 |
| 10 | 527176 | 262400 | -1299.85 | 2321.01 | 1.98 % | 1.83 % | 77.5 | 7.50 |

The case uses the 20 mm mesh. Its error is 3.8 % and 3.4 %.
The tolerance holds this error and the 0.2 % error of the reference, with a margin of 1 %.
A wrong load, a wrong support or a wrong unit gives a much larger error.

## Time and memory limits

The time and the memory are from `/usr/bin/time -l` on a laptop with 16 GB of memory (Apple M5, 10 cores), with CalculiX 2.23 and SPOOLES.
The time is the wall time. The memory is the maximum resident set size.

The case has `time_limit_s = 120` and `memory_limit_mib = 6144`.
Thus `fh validate` stops the case if the solve becomes 10 times slower or needs 2 times more memory.
The 10 mm mesh needs 7.5 GiB, and a 16 GB laptop can also solve it.
