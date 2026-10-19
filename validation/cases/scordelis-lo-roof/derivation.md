# Scordelis-Lo roof with S6 shells: derivation

This case checks the shell path: `fh mesh --shell`, the `S6` element, `*SHELL SECTION` and `ccx`.
The roof is a cylinder shell with a published deflection.

## Source

MacNeal, R.H. and Harder, R.L., "A proposed standard set of problems to test finite element
accuracy", Finite Elements in Analysis and Design 1 (1985), pages 3 to 20. The problem is the
Scordelis-Lo roof. The published target is a vertical deflection of 0.3024 at the middle of the
free edge. (The first solution of Scordelis and Lo gave 0.3086.)

## Data

The published problem has the units of its paper. This case uses mm and MPa. Every length is 1000
times larger, and the stress is the same ratio to the load. Thus the deflection is 1000 times
larger too: 0.3024 becomes 302.4 mm.

| Item | Published | This case |
|---|---|---|
| Radius R | 25 | 25000 mm |
| Length L | 50 | 50000 mm |
| Half angle | 40 degrees | 40 degrees |
| Thickness t | 0.25 | 250 mm |
| Young modulus E | 4.32E8 | 432 MPa |
| Poisson ratio | 0 | 0 |
| Self weight, per area of shell | 90 | 9.0E-5 MPa |

The weight ratio to E is the same in both columns: 90 / 4.32E8 = 9.0E-5 / 432 = 2.083E-7.
In the job, the weight is the density 3.6E-7 t/mm3 with gravity 1 mm/s2 in -z.
This gives 3.6E-7 x 1 x 250 = 9.0E-5 MPa.

## Model

Axes: x along the roof, z up, the axis of the cylinder is the x axis. The crown is at y = 0.
The model is the full roof. It has no symmetry plane, because a rotation constraint on
the mid-length plane made the roof 9 times too stiff in `ccx` (S6, uz = -34 mm).

- Each end (x = 0 and x = 50000) is a rigid diaphragm in its plane: uy = uz = 0, ux free.
- The 2 curved edges at y = +/-16069.7 mm are free.
- ux = 0 at the node TIP blocks the rigid motion along x. By symmetry, ux is 0 at TIP.
- TIP is the node at x = 25000, y = 16069.69 mm, z = 19151.11 mm, on the free edge at mid-length.

## Mesh of the case

The STEP file comes from `part.py` (build123d through `fh-python`).
The mesh file `part.inp` comes from these commands in this folder. `job.inp` includes it.

```
fh-python part.py
fh mesh part.step --units SI-mm --shell --size 1250 -o part.inp \
  --nodes END0=x:0 --nodes END1=x:50000 \
  --nodes TIP=point:25000,16069.69024216,19151.11107797
```

The mesh has 6033 nodes and 2948 S6 elements.

## Result

| Mesh size | Nodes | uz at TIP | Error |
|---|---|---|---|
| 5000 mm | 423 | -282.9 mm | -6.4 % |
| 2500 mm | 1513 | -299.2 mm | -1.1 % |
| 1250 mm (this case) | 6033 | -301.5 mm | -0.3 % |
| 625 mm | 23809 | -301.8 mm | -0.2 % |

The reference in `check.toml` is -302.4 mm.

## Tolerance

The tolerance is 1 percent. The mesh error of this case is 0.3 percent, and the published value has
4 digits. The remaining margin is for a change of mesh or solver version.
