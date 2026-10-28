# dynamics-bar-step derivation

## Conclusion

The solid bar gives the end displacement at t = 2L/c 1.57 % below the 1D wave theory of Graff (1975). The mean u1 of the end face is 0.0187484 mm, and the theory gives 0.0190476 mm. The tolerance is 3 %.

At t = 2L/c = 77.337 µs, the wave comes back from the fixed end, and the 1D theory has a sharp peak. The 3D bar has lateral inertia, and the mesh cannot carry a step. Thus the solid has a round peak with a lower value.

The mesh causes only about 0.2 % of the difference: the 2 finest former C3D10 meshes of derivation.md give -1.36 % and -1.38 %. The run prints u1 only at the end of the step, after 504 explicit increments. Thus this report has no plot of the history.

## Result

**Pass.** A check passes when |computed - reference| ≤ rel_tol · |reference| + abs_tol, with rel_tol = 0.03 and abs_tol = 0.0 (`check.toml`). `fh validate` gives the computed values.

| Check | Unit | Computed | Reference | Difference | Tolerance | Margin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `u_end` | Not specified | 0.0187484 | 0.0190476 | -0.000299165 (-1.57062 %) | 0.0005714 | 0.0002723 |

Made by `scripts/validation-reports.py` on 2026-10-28, femahoy 5ac0a46.

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

## Mesh study

Each mesh is a former C3D10 mesh of `fh mesh` with another `--size`. The model is the same as in `job.inp`.
ccx uses its stable increment, thus a finer mesh also has a smaller time increment.

| Size (mm) | Elements | Increment (s) | Time | u1 (mm) | Difference |
|---:|---:|---:|---:|---:|---:|
| 10 | 408 | 5.32e-8 | 4 s | 0.0186896 | -1.88 % |
| 7 | 556 | 7.34e-8 | 4 s | 0.0187101 | -1.77 % |
| 5 (former case) | 794 | 4.64e-8 | 7 s | 0.0187435 | -1.60 % |
| 3.3 | 3830 | 2.08e-8 | 33 s | 0.0187887 | -1.36 % |
| 2.5 | 7256 | 1.86e-8 | 139 s | 0.0187850 | -1.38 % |

The times are the times of `fh validate` on a Mac with 10 cores and other work.
Along the bar, each mesh has about 200/size elements. The sizes 10, 7 and 5 have only 1 or 2 elements across the section. Thus they have almost the same number of elements.
The 2 finest meshes differ by 0.02 %. Thus the mesh error of this case is about 0.23 % (0.00004 mm), the difference from the mean of the 2 finest meshes.
The other 1.37 % comes from the 3D bar, not from the mesh: the 3D bar has lateral inertia. The implicit chat run with size 2.5 mm gives -1.33 % ("Tolerance").

## Tolerance

The tolerance is 3 % (`rel_tol = 0.03`). It holds the mesh error of about 0.23 % and the 1.37 % of the 3D bar ("Mesh study").
The peak of the 1D theory has a sharp corner at t = 2L/c. The 3D bar has lateral inertia, and the mesh cannot carry a step. Thus the corner is round and the peak is lower:

| Model | u1 (mm) | Difference |
|---|---:|---:|
| Former case: explicit, size 5 mm, at 2L/c | 0.0187435 | -1.60 % |
| Chat run: implicit, size 2.5 mm, 0.25 µs, the peak at 77.75 µs | 0.0187947 | -1.33 % |

Before the corner, the chat run agrees with the 1D theory: at t = 50 µs it gives 0.012309 mm against 0.012315 mm (-0.05 %).

The chat used `*DYNAMIC, DIRECT` (implicit). For the former 5 mm mesh, an implicit increment took about 0.2 s, so 77 increments took 16 s.
`*DYNAMIC, EXPLICIT` took 6 s. ccx used its stable increment, 4.64e-8 s, in place of the 1e-7 s of the input.

## Chat run

The request asked for the largest end displacement in the first 100 µs in plain words.
The chat gave 0.0188 mm at 77.5 µs (-1.3 %). The run took 1876 s and cost 0.99 USD.
Most of the time was the last solve: 400 implicit increments of 0.25 µs. The chat had no failure of the model.

## Files

Run `python3 mesh.py` to write `mesh.inp` from `mesh.geo` with gmsh.
The transfinite faces and the extrusion use `Recombine` and `Mesh.ElementOrder=1`.
The mesh has 80 x 4 x 4 C3D8R elements, 1280 elements and 2025 nodes.
The mesh retains the 200 x 10 x 10 mm box, `FIXED` at x = 0 and `END` at x = 200.
The `END` surface uses the outward S4 face of each end element.
`job.inp` includes the mesh and keeps the original material, explicit step, pressure, check and 3 % tolerance.

Reason: The audit mesh gives the same accuracy with a shorter solver time and a uniform stable increment.
`fh validate` on the Mac gives 0.01874844 mm, a difference of -1.57 % from the reference.
The C3D10 study above records the former mesh. The new result differs from the former case by less than 0.03 %.
