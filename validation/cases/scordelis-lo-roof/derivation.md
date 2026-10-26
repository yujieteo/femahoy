# Scordelis-Lo roof with S8R shells: derivation

This case checks the mapped `S8R` shell, `*SHELL SECTION` and `ccx`.
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

Run `python3 mesh.py` to write `part.inp` from `mesh.geo` with gmsh.
The mesh has 24 elements along x and 24 elements across the circular section.
Two circular arcs share the crown. `Extrude`, `Layers` and `Recombine` form the shell faces.
`Mesh.ElementOrder=2` and `Mesh.SecondOrderIncomplete=1` produce 8-node quadrilaterals.
The middle nodes stay on the cylinder.
The mesh has 1825 nodes and 576 S8R elements.
`job.inp` includes `part.inp` and keeps the original material, constraints and gravity load.

Reason: The mapped audit mesh converges with 1825 nodes. The former S6 mesh needs 6033 nodes and gives -301.535 mm.
The former S6 inputs, `part.py` and `part.step`, are in the git history before this change.

## Mesh convergence

Run `python3 mesh.py N` for each even count N, then run `fh validate` on a copy of this case.
These results use gmsh 4.15.2 and CalculiX 2.23 on the Mac.

| N x N | Elements | Nodes | uz at TIP (mm) |
|---|---:|---:|---:|
| 16 x 16 | 256 | 833 | -301.9568 |
| 24 x 24, this case | 576 | 1825 | -301.9235 |
| 32 x 32 | 1024 | 3201 | -301.9209 |

The 24 x 24 result differs from the 32 x 32 result by 0.001 %.
The reference in `check.toml` is -302.4 mm. The case differs from the reference by -0.158 %.

## Tolerance

The tolerance stays at 1 percent. The published value has 4 digits.
The mapped mesh study gives a smaller error than the former S6 study.
