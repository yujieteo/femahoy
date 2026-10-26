# random-vibration-cantilever derivation

This case checks the random vibration path: a steel cantilever on a shaker with a flat base acceleration PSD.
The check is the RMS acceleration of the tip.
The reference is the modal sum of the beam theory, and Miles' equation is the hand check.

## Why this case

No other case of the suite has any of these items:

- A random response: a PSD of the base acceleration, and the RMS of the response.
- `*STEADY STATE DYNAMICS`: the frequency response from the modes, with `*MODAL DAMPING`.
- `*BASE MOTION`: the support moves, and the response is the absolute motion.
- The RMS step of `fh` (`crates/fh/src/ccx/rms.rs`): `ccx/rms.txt` and the check field `A_RMS`.

## Sources

- J. W. Miles, "On structural fatigue under random loading", Journal of the Aeronautical Sciences, vol. 21, no. 11, 1954, pp. 753-762.
  It gives the RMS response of 1 mode to a flat PSD: Miles' equation.
- R. D. Blevins, *Formulas for Natural Frequency and Mode Shape*, Van Nostrand Reinhold, New York, 1979, Table 8-1, the clamped-free beam.
  It gives λ_r and σ_r of the mode shapes.

The derivation below gives the reference from these 2 sources, so that a reader can check it without a book.

The beam has the length L, the bending stiffness EI and the mass per length m.
The base moves with the acceleration a_b(t). The deflection w(x, t) relative to the base obeys:

```
EI w'''' + m w_tt = -m a_b
phi_r(x) = cosh(lam_r x / L) - cos(lam_r x / L) - sig_r (sinh(lam_r x / L) - sin(lam_r x / L))
omega_r = lam_r^2 / L^2 sqrt(EI / m)
```

With this scale of phi_r: the integral of phi_r^2 over the beam is L, the integral of phi_r is 2 sig_r L / lam_r, and phi_r(L) = 2 (-1)^(r+1).
Each mode r with the damping ratio zeta obeys q_tt + 2 zeta omega_r q_t + omega_r^2 q = -G_r a_b, with G_r = 2 sig_r / lam_r.
G_r^2 is the fraction of the mass that mode r moves (the effective modal mass).

The acceleration of the tip is the base acceleration plus the acceleration of w at x = L. For a base acceleration exp(j omega t), it is:

```
T(f) = 1 + omega^2 sum_r c_r / (omega_r^2 - omega^2 + 2 j zeta omega_r omega)
c_r = G_r phi_r(L) = 4 sig_r (-1)^(r+1) / lam_r,  omega = 2 pi f
A_rms^2 = integral from f1 to f2 of |T(f)|^2 W df
```

| Item | Value |
|---|---|
| Beam | L = 100 mm, 10 mm in y, 5 mm in z. I = 10 x 5^3 / 12 = 104.17 mm4 |
| Steel | E = 210000 MPa, rho = 7.85e-9 t/mm3: m = 3.925e-7 t/mm |
| Base acceleration | in z. PSD W = 0.04 g^2/Hz, flat from 20 to 2000 Hz. The input is 8.90 g RMS. g = 9806.65 mm/s2 |
| Damping | zeta = 0.02 in each mode (Q = 1 / (2 zeta) = 25) |

The modes that bend the beam in z:

| r | λ_r | σ_r | f_r (Hz) | c_r | G_r^2 |
|---:|---:|---:|---:|---:|---:|
| 1 | 1.87510407 | 0.734095514 | 417.76 | 1.56598 | 0.6131 |
| 2 | 4.69409113 | 1.018467319 | 2618.0 | -0.86787 | 0.1883 |
| 3 | 7.85475744 | 0.999224497 | 7330.6 | 0.50885 | 0.0647 |
| 4 | 10.99554073 | 1.000033553 | 14365 | -0.36380 | 0.0331 |
| 5 | 14.13716839 | 0.999998550 | 23746 | 0.28294 | 0.0200 |

For r above 5, λ_r = (2r - 1) π / 2 and σ_r = 1 to 7 digits.
The sum of c_r is 1: the rigid motion of the beam.
This Python code integrates |T|^2 with 40 modes and Simpson's rule on 400000 intervals of log f:

```python
import math
E, rho, b, h, L = 210000.0, 7.85e-9, 10.0, 5.0, 100.0  # SI-mm
zeta, g, W = 0.02, 9806.65, 0.04          # damping ratio, g in mm/s2, PSD in g^2/Hz
I, m = b * h**3 / 12, rho * b * h
lam = [1.87510407, 4.69409113, 7.85475744, 10.99554073, 14.13716839]
sig = [0.734095514, 1.018467319, 0.999224497, 1.000033553, 0.999998550]
lam += [(2 * r - 1) * math.pi / 2 for r in range(6, 41)]
sig += [1.0] * 35
modes = [(l * l / L**2 * math.sqrt(E * I / m), 4 * s / l * (-1) ** r)
         for r, (l, s) in enumerate(zip(lam, sig))]  # (omega_r, c_r)
def T(f):  # tip acceleration / base acceleration
    w = 2 * math.pi * f
    return 1 + w * w * sum(c / (wr * wr - w * w + 2j * zeta * wr * w) for wr, c in modes)
N, a, z = 400000, math.log(20), math.log(2000)  # Simpson's rule in log f
s = sum((1 if k in (0, N) else 4 if k % 2 else 2) * f * abs(T(f)) ** 2
        for k in range(N + 1) for f in [math.exp(a + (z - a) * k / N)])
A = math.sqrt(W * s * (z - a) / N / 3) * g
print(f"A_rms = {A:.1f} mm/s2 = {A / g:.4f} g")
```

It prints `A_rms = 400925.4 mm/s2 = 40.8830 g`: the value of the check `a_tip_rms`.
With 5 modes the value is 0.001 % lower, and with 800000 intervals it does not change.

Miles' equation for mode 1 alone is the hand check: A_rms = c_1 sqrt(π/2 f_1 Q W) = 1.56598 x sqrt(π/2 x 417.76 x 25 x 0.04) = 40.115 g.
It is 1.9 % lower than the modal sum. The modal sum also has the base acceleration below the resonance and the static part of the higher modes.

## Model

| Item | Value |
|---|---|
| Mesh | `mesh.inp`: mapped C3D20R, 40 x 4 x 2 elements, 2117 nodes. The same mesh as `frequency-cantilever` |
| Support | `FIXED`, the face x = 0: `*BOUNDARY` in x, y and z |
| Step 1 | `*FREQUENCY, STORAGE=YES`, 10 modes, up to 18.2 kHz. `STORAGE=YES` keeps the modes for step 2. `*NODE FILE` writes the modes for the page |
| PSD | `*AMPLITUDE, NAME=PSD`: sqrt(W) = 0.2 g = 1961.33 mm/s2 at all frequencies |
| Step 2 | `*STEADY STATE DYNAMICS`: 20 to 2000 Hz, 101 points in each interval, bias 1 |
| Damping | `*MODAL DAMPING`: modes 1 to 10, zeta = 0.02 |
| Base motion | `*BASE MOTION, DOF=3, TYPE=ACCELERATION, AMPLITUDE=PSD` on `FIXED` |
| Output | `*NODE PRINT` of `U` for `TIP`, the 37 nodes of the face x = 100 mm. `*NODE FILE, FREQUENCY=0` stops the field output of step 1 in step 2 |

`ccx` puts the frequency points in the intervals between 20 Hz, each natural frequency in the band (419.1 Hz and 831.9 Hz) and 2000 Hz: 301 points.
At each point it prints the real part and the imaginary part of `U`. With the amplitude sqrt(W), |U(f)|^2 is the PSD of the displacement.
`fh` integrates (2 π f)^4 |U|^2 over the points with the trapezoid rule and writes the RMS acceleration of each node to `ccx/rms.txt`.
The check `a_tip_rms` is the mean of `A3` over the nodes of `TIP`. The former 57 values differ by less than 0.002 %.

The step-1 output has the effective modal mass of each mode in z. It agrees with G_r^2 of the beam theory:

| Mode of the solid | f (Hz) | Beam f (Hz) | Fraction of the mass in z | Beam G_r^2 |
|---:|---:|---:|---:|---:|
| 1 | 419.12 | 417.76 | 0.6150 | 0.6131 |
| 3 | 2596.8 | 2618.0 | 0.1902 | 0.1883 |
| 6 | 7145.6 | 7330.6 | 0.0659 | 0.0647 |

Mode 2 (831.9 Hz) bends the beam in y. The base motion in z does not move it.

## Frequency points

The trapezoid rule needs points that are close at each resonance. The half-power width of mode 1 is 2 zeta f_1 = 16.8 Hz.
The values below use the former C3D10 check mesh, 10 modes and bias 1. h is the spacing of the points near mode 1.

| Points in each interval | h (Hz) | h / (zeta f_1) | A_rms (mm/s2) | Change to 201 points |
|---:|---:|---:|---:|---:|
| 21 | 20.0 | 2.38 | 431274.5 | +7.49 % |
| 41 | 10.0 | 1.19 | 403402.8 | +0.540 % |
| 61 | 6.7 | 0.79 | 401400.2 | +0.041 % |
| 101, the check | 4.0 | 0.48 | 401236.1 | +0.0006 % |
| 201 | 2.0 | 0.24 | 401233.8 | 0 |

With 21 points and bias 3, the points near mode 1 are 7.3 Hz apart, but the value is +0.68 % (403939.8).
A spacing h of zeta f_1 / 2 or less, with bias 1, makes this error negligible.

## Former C3D10 mesh study

The meshes come from `fh mesh part.step --size H -o mesh.inp --face FIXED=x:0 --nodes TIP=x:100`, with 101 points and 10 modes.

| H (mm) | Nodes | f_1 (Hz) | A_rms (mm/s2) | Difference to the reference | Time |
|---:|---:|---:|---:|---:|---:|
| 5 | 939 | 419.73 | 401397.3 | +0.118 % | 1.5 s |
| 3.3, the former check | 2263 | 419.32 | 401236.1 | +0.078 % | 3.5 s |
| 2.5 | 3969 | 419.20 | 401195.0 | +0.067 % | 9.0 s |
| 1.67 | 11154 | 418.99 | 401110.7 | +0.046 % | 26 s |
| 1.25 | 22303 | 418.90 | 401075.7 | +0.037 % | 58 s |

The times are the times of `ccx` 2.23 on a shared Mac with 10 cores.
The value goes down as the mesh becomes finer, to about +0.02 %. That is the difference between the solid and the beam theory: shear, rotary inertia and the Poisson effect.

With the former C3D10 check mesh and 101 points, 5 modes give +0.146 % and 20 modes give +0.074 %.

## Tolerance

The tolerance is `rel_tol = 0.005` and `abs_tol = 0`.

| Cause | Basis | Effect |
|---|---|---:|
| The reference | the integral and the sum of 40 modes converge to 1e-6 | 0 |
| Solid and beam theory | the trend of the mesh study | +0.02 % |
| Mesh error of the check mesh | 3.3 mm against 1.25 mm | +0.04 % |
| Frequency points | 101 against 201 points | +0.001 % |
| Modes | 10 against 20 modes | +0.003 % |
| Another mesh of the same size | The former free mesh varies with the gmsh version. The mapped mesh fixes the grid. The original budget stays conservative. | +0.12 % or less |

`rel_tol` = 0.5 % is 4 times the largest of these. The check value is far from 0, thus `abs_tol` is 0.

| Check | Model | Reference | Difference | Tolerance | Part of the tolerance |
|---|---:|---:|---:|---:|---:|
| `a_tip_rms` | 401161.7 | 400925.4 | +236.3 | 2004.6 | 12 % |

The tolerance stops these errors. Each line is a change of `job.inp`, solved with `fh validate`, or a hand calculation:

| Wrong model | A_rms (mm/s2) | Error |
|---|---:|---:|
| Miles' equation, mode 1 only (hand calculation) | 393396.4 | -1.88 % |
| 41 points in each interval, h = 1.19 zeta f_1 | 403402.8 | +0.62 % |
| 21 points in each interval with bias 3 | 403939.8 | +0.75 % |
| An inertia load `*DLOAD` `GRAV` in place of `*BASE MOTION`: the relative acceleration | 424588.9 | +5.90 % |
| zeta = 0.025 | 360508.7 | -10.1 % |
| The PSD as the amplitude, not its square root | 7.871e8 | 1963 times |

## Not in the check

- The deflection relative to the base: with `*BASE MOTION`, `U` is the absolute displacement, and its RMS (0.335 mm at the tip) is mostly the motion of the base. The beam theory gives 0.0571 mm RMS for the deflection of the tip.
- The stress: `ccx` prints no stress to `job.dat` in a steady state step.
- A PSD that is not flat: `ccx` interpolates the amplitude linearly between its points. The flat PSD of the check has no interpolation error.

On a Mac with 10 cores, `fh run` of the case took 3.5 s.

## Mapped mesh

Run `python3 mesh.py` to write `mesh.inp` from `mesh.geo` with gmsh.
The box uses the same mapped mesh as `frequency-cantilever`: 320 C3D20R elements and 2117 nodes.
The transfinite faces and the extrusion use `Recombine`, `Mesh.ElementOrder=2` and `Mesh.SecondOrderIncomplete=1`.
The mesh retains `FIXED` at x = 0 and `TIP` at x = 100 mm.

Reason: The audit shows fewer nodes for the same accuracy. The mapped grid also removes the free mesh choice.
`fh validate` on the Mac gives 401161.7 mm/s2, a difference of +0.059 % from the reference.
The frequency steps, PSD, damping, base motion, check and 0.5 % tolerance stay the same.
The earlier frequency-point and mode studies above use the former C3D10 mesh.
