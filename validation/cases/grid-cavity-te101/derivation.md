# grid-cavity-te101 derivation

This case checks the resonant mode solve of the built-in electromagnetic solver: the lowest mode of a closed rectangular cavity with copper walls. The solver uses edge elements on tetrahedra and the Leontovich surface impedance on the walls (`fh guide grid`). The case is not a chip package problem. Units are SI-mm.

## Source

A rectangular cavity of a × b × d with perfect walls has the TE10l modes at f = (c / 2) sqrt((1 / a)² + (l / d)²). For l = 1, walls of the surface resistance Rs = sqrt(ω μ0 / (2σ)) give Q = (k a d)³ b η / (2π² Rs) / (2a³b + 2bd³ + a³d + ad³). D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley, 2012, section 6.3.

A wall with the surface impedance Zs = Rs (1 + j) has a surface reactance equal to its resistance. The reactance moves the frequency down by f / (2Q), so the complex frequency is f (1 + (j − 1) / (2Q)). J. D. Jackson, *Classical Electrodynamics*, 3rd ed., Wiley, 1999, section 8.8.

## Model

| Item | Value |
|---|---|
| Cavity | a = 22.86 mm (x), b = 10.16 mm (y), d = 25 mm (z), air |
| Walls | copper, σ = 5.8e7 S/m, on all 6 faces |
| Step | modes near 8 GHz, 1 mode |

f(TE101) = 8.885173 GHz with perfect walls. Rs = 0.024592 ohm and Q = 7815.46. The frequency with the copper walls is 8.885173 GHz × (1 − 1 / 15630.92) = 8.884604442 GHz.

| Check | Formula | Value |
|---|---|---|
| `frequency` | f (1 − 1 / (2Q)) | 8.884604442e9 Hz |

## Mesh study

| max_cell (mm) | points | unknowns | frequency (Hz) | error | Q | error of Q | time |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 4 | 288 | 1,527 | 8.8877966e9 | 3.6e-4 | 7681.4 | −1.7 % | 0.03 s |
| 2 | 1,890 | 11,448 | 8.8858451e9 | 1.4e-4 | 7783.1 | −0.41 % | 0.8 s |
| 1 | 13,364 | 87,055 | 8.8850940e9 | 5.5e-5 | 7807.2 | −0.11 % | 37 s |

Both errors fall at second order. The solver finds each mode first with the wall terms at the frequency `near`, then again at the frequency of the mode, until the frequency stops changing. With the wall terms at 8 GHz only, the Q at max_cell 2 mm was 7385, 5 % low.

## Tolerance

The check uses max_cell 2 mm. The error of the frequency is 1.4e-4. The tolerance is 3e-4 of the value.
