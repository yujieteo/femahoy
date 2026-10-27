# grid-waveguide-step derivation

This case checks the built-in electromagnetic solver at a change of shape: a symmetric H-plane step from a wide to a narrow rectangular waveguide. The step reflects a part of the TE10 wave and excites evanescent TE30 and higher modes near the step. No closed form exists, so the reference is the GetDP solution of the same model with an unstructured mesh (`getdp/`). The case is not a chip package problem. Units are SI-mm.

## Source

The reference solves the curl-curl equation of the electric field with first-order edge elements in GetDP 4.0, with the first-order TE10 port condition on each port face (J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley, 2014, section 8.5). The S-parameters are the projections of the field on the TE10 mode of each port, normalized to waves of 1 W (D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley, 2012, sections 3.3 and 4.3). The GetDP model and its mesh are independent of femahoy: gmsh makes the tetrahedra from the 2 boxes, and `getdp/case.pro` holds the equations.

To repeat the reference: `gmsh -3 getdp/case.geo -o case.msh`, then `getdp getdp/case.pro -msh case.msh -solve Wave -pos Wave`. The files `p1.txt` and `p2.txt` hold S11 + 1 and S21 (real, imaginary).

## Model

| Item | Value |
|---|---|
| Wide guide | WR-90, x = 0 to 22.86 mm, y = 0 to 10.16 mm, z = 0 to 25 mm, air |
| Narrow guide | x = 3.43 to 19.43 mm (16 mm wide), y = 0 to 10.16 mm, z = 25 to 50 mm, air |
| Walls | perfect conductors, also the face of the step at z = 25 mm |
| Ports | TE10 at z = 0 (port 1) and z = 50 mm (port 2), 25 mm from the step |
| Frequency | 10 GHz: TE10 propagates in both guides (cut-off 6.557 and 9.369 GHz) |

The TE30 mode, the first mode that the symmetric step excites after TE10, decays by exp(−8.9) in the wide guide and by more in the narrow guide over the 25 mm to the port. So a TE10 port condition is sufficient at both ports.

## Mesh study

| Solver | cell (mm) | points | S11 | S21 | time |
|---|---:|---:|---|---|---:|
| GetDP | 1.0 | 9,910 | 0.20722 − 0.29726 j | 0.73719 + 0.56815 j | 2.6 s |
| GetDP | 0.7 | 26,092 | 0.20300 − 0.30185 j | 0.73677 + 0.56886 j | 12 s |
| GetDP | 0.5 | 66,053 | 0.19963 − 0.30459 j | 0.73749 + 0.56820 j | 57 s |
| femahoy | 2.0 | 3,289 | 0.23048 − 0.28883 j | 0.71971 + 0.58733 j | 0.4 s |
| femahoy | 1.4 | 8,919 | 0.21635 − 0.29558 j | 0.72885 + 0.57836 j | 3.9 s |
| femahoy | 1.0 | 22,954 | 0.20770 − 0.30120 j | 0.73225 + 0.57436 j | 35 s |

The reference is the GetDP solution at 0.5 mm. The difference of femahoy from it at 2.0, 1.4 and 1.0 mm is 0.031, 0.017 and 0.0081 in the real part of S11, the largest part. Both solvers move toward 1 answer when the cell gets smaller. \|S11\|² + \|S21\|² is 0.9988 for GetDP at 0.5 mm and 0.9998 for femahoy at 1.0 mm (the exact value is 1).

## Tolerance

The check uses max_cell 1.4 mm. The largest difference from the reference is 0.0168. The pass rule uses abs_tol 0.035 for each part. At 1 mm the difference is 0.0081, but the solve takes 35 s, too long for the test of each grid case in CI.
