# buckling-column derivation

## Conclusion

The solid model gives the critical load of the column 0.10 % above the theory of Timoshenko and Gere (1961). The model gives 10806.05 N, and the theory gives 10794.88 N. The tolerance is 1 %.

Mode 2 is at 10806.44 N, 0.004 % above mode 1. The square section has the same critical load in 2 directions, as derivation.md says.

A finer mesh gives +0.06 %: the size 3.3 mm in derivation.md, and the size 2.5 mm in the chat run. Thus about 0.06 % is the difference between the solid and the theory, not a mesh error. The theory has no shear deformation, and the fixed base face of the solid stops the Poisson contraction there (derivation.md).

## Result

**Pass.** A check passes when |computed - reference| ≤ rel_tol · |reference| + abs_tol, with rel_tol = 0.01 and abs_tol = 0.0 (`check.toml`). `fh validate` gives the computed values.

| Check | Unit | Computed | Reference | Difference | Tolerance | Margin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `p_cr` | Not specified | 10806 | 10794.9 | +11.17 (+0.103475 %) | 107.9 | 96.78 |

Made by `scripts/validation-reports.py` on 2026-10-28, femahoy 9c54611.

This case checks the critical load of a steel column that is fixed at the base and free at the top (units SI-mm).

## Source

S. P. Timoshenko and J. M. Gere, *Theory of Elastic Stability*, 2nd ed., McGraw-Hill, New York, 1961, Art. 2.1, the column with the lower end built in and the upper end free: P_cr = π² E I / (4 L²).

## Model

| Item | Value |
|---|---|
| Column | 10 x 10 mm section, 200 mm long in z |
| Steel | E = 210000 MPa, nu = 0.3 |
| Support | `BASE`, the face z = 0: all 3 displacements are 0 |
| Load | `TOP`, the face z = 200: pressure 0.01 MPa, a force of 1 N in -z |
| Step | `*BUCKLE`, 4 modes |

The load is 1 N. Thus the buckling factor is the critical force in N.
I = 10⁴ / 12 = 833.33 mm4. P_cr = π² · 210000 · 833.33 / (4 · 200²) = 10794.88 N.
The section is square, so modes 1 and 2 have the same load in 2 directions.

## Tolerance

The tolerance is 1 % (`rel_tol = 0.01`).
The solid gives 10806.05 N (+0.10 %) with a mesh of size 5 mm, and 10801.55 N (+0.06 %) with size 3.3 mm.
The theory has no shear deformation. The fixed base face of the solid also stops the Poisson contraction at the base. Both effects are small for this column: L / r = 69.

## Chat run

The request asked for the critical load of this column in plain words.
The chat wrote a `*BUCKLE` input with a 1 N load and a mesh of size 2.5 mm (13256 nodes). It gave 10801.4 N (+0.06 %).
The run took 65 s and cost 0.34 USD. The chat had no failure. This case uses the input of the chat with a coarser mesh.

## Files

The mesh in `job.inp` comes from `fh mesh part.step --size 5 --face BASE=z:0 --face TOP=z:200`.
`part.step` is a box of 10 x 10 x 200 mm with a corner at the origin.
