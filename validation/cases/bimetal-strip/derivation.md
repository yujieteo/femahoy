# bimetal-strip derivation

A strip of 80 x 4 x 2 mm has 2 layers of 1 mm each (units SI-mm).
The temperature of all nodes changes from 20 to 120, thus the change is 100.

| Layer | z (mm) | E (MPa) | nu | alpha (1/K) |
|---|---|---|---|---|
| 1, material `LOW` | 0 to 1 | 200000 | 0 | 1.2e-5 |
| 2, material `HIGH` | 1 to 2 | 70000 | 0 | 2.3e-5 |

## Model

The mesh is a quarter of the strip: x in [0, 40], y in [0, 2], z in [0, 2].
The planes x = 0 and y = 0 are planes of symmetry (u1 = 0 and u2 = 0).
Node `ORIGIN` at (0, 0, 0) has u3 = 0. Thus the strip has no rigid motion.
`scripts/gen-bimetal-strip.py` writes `strip.inp` with C3D8I elements of 1 x 1 x 0.5 mm.

## Reference

Far from the free end x = 40, the axial strain is linear through the thickness: exx = eps0 + kappa z.
The stress in layer i is sxx = E_i (eps0 + kappa z - alpha_i dT).
Because nu = 0, the stress sxx does not change the other strains.
The axial force and the bending moment on a section are 0:

- sum over the layers of the integral of sxx dz = 0
- sum over the layers of the integral of sxx z dz = 0

These 2 linear equations give eps0 = 659/730000 and kappa = 7/9125 1/mm.
The formula of Timoshenko (1925) gives the same kappa, with m = t1/t2 = 1, n = E1/E2 and h = 2 mm:

kappa = 6 (alpha2 - alpha1) dT (1 + m)^2 / (h (3 (1 + m)^2 + (1 + m n) (m^2 + 1 / (m n))))

The displacement is u1 = (eps0 + kappa z) x and u3 = -kappa x^2 / 2 + f(z), with u3 = 0 at the origin.
At node `P20` (x = 20, y = 0, z = 0):

- u3 = -200 kappa = -0.15342465753424658 mm
- u1 = 20 eps0 = 0.018054794520547944 mm

## Tolerance

The free end has a boundary layer of about the thickness, 2 mm. Node `P20` is 20 mm from the free end.
C3D8I gives pure bending exactly on a regular mesh. Thus only the 6 significant digits of `job.frd` remain.
That rounding is at most 5e-6 of the value. The tolerance rel_tol = 1e-5 covers it.
