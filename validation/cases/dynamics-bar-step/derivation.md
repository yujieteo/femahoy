# dynamics-bar-step derivation

This case checks the end displacement of a steel bar after a sudden end force (units SI-mm).

## Source

The 1D theory of longitudinal waves in a thin bar: K. F. Graff, *Wave Motion in Elastic Solids*, Clarendon Press, Oxford, 1975, chapter 2 (longitudinal waves in thin rods).
A step force F on the free end sends a wave with the speed c = sqrt(E / rho). The end moves with the constant speed F / (A sqrt(E rho)).
The wave comes back from the fixed end at t = 2L/c. Then the end is at 2 F L / (E A), 2 times the static displacement, and starts to move back.

## Model

| Item | Value |
|---|---|
| Bar | 200 mm in x, 10 x 10 mm section |
| Steel | E = 210000 MPa, nu = 0.3, rho = 7.85e-9 t/mm3 |
| Support | `FIXED`, the face x = 0: all 3 displacements are 0 |
| Load | `END`, the face x = 200: pressure -10 MPa (a pull of 1000 N in +x) from t = 0 |
| Step | `*DYNAMIC, EXPLICIT` to t = 77.337 µs |

c = 5172 m/s and 2L/c = 77.337 µs. The reference is 2 · 1000 · 200 / (210000 · 100) = 0.0190476 mm.
The check is the mean u1 of the face `END` at the end of the step.

## Tolerance

The tolerance is 3 % (`rel_tol = 0.03`).
The peak of the 1D theory has a sharp corner at t = 2L/c. The 3D bar has lateral inertia, and the mesh cannot carry a step. Thus the corner is round and the peak is lower:

| Model | u1 (mm) | Difference |
|---|---:|---:|
| This case: explicit, size 5 mm, at 2L/c | 0.0187435 | -1.60 % |
| Chat run: implicit, size 2.5 mm, 0.25 µs, the peak at 77.75 µs | 0.0187947 | -1.33 % |

Before the corner, the chat run agrees with the 1D theory: at t = 50 µs it gives 0.012309 mm against 0.012315 mm (-0.05 %).

The chat used `*DYNAMIC, DIRECT` (implicit). For this mesh an implicit increment takes about 0.2 s, so 77 increments take 16 s.
`*DYNAMIC, EXPLICIT` takes 6 s. ccx uses its stable increment, 4.64e-8 s, in place of the 1e-7 s of the input.

## Chat run

The request asked for the largest end displacement in the first 100 µs in plain words.
The chat gave 0.0188 mm at 77.5 µs (-1.3 %). The run took 1876 s and cost 0.99 USD.
Most of the time was the last solve: 400 implicit increments of 0.25 µs. The chat had no failure of the model.

## Files

The mesh in `job.inp` comes from `fh mesh part.step --size 5 --face FIXED=x:0 --face END=x:200`.
`part.step` is a box of 200 x 10 x 10 mm with a corner at the origin.
