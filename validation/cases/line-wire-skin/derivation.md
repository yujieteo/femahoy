# line-wire-skin derivation

This case checks the eddy currents of the transmission line solver (`fh guide grid`, a `[line]` table): the skin effect in a round copper wire. The case is not a chip package problem. Units are SI-m.

## Source

In a round wire of radius a and conductivity σ with the time factor e^(jωt), the current density is J(r) ∝ J0(kr) with k = (1 − j) / δ and the skin depth δ = sqrt(2 / (ω μ0 σ)). The internal impedance per length is Z = k J0(ka) / (2π a σ J1(ka)). S. A. Schelkunoff, The electromagnetic theory of coaxial transmission lines and cylindrical shields, Bell System Technical Journal 13 (1934) 532-579 (the internal impedance of a round wire). R is the real part of Z. A field outside the wire adds only an inductance, so R does not depend on the box. The value below is from mpmath at 30 digits, with μ0 = 1.25663706212e-6 H/m.

## Model

| Item | Value |
|---|---|
| Wire | copper, σ = 5.8e7 S/m, radius a = 1 mm, centre (0, 0), the signal |
| Air | square, x and y from −10 mm to 10 mm |
| Walls | all 4 sides of the box are electric walls (the return path) |
| Frequencies | 1 kHz, 10 kHz, 100 kHz and 1 MHz; the check reads the last row |

| f (Hz) | a / δ | R (Ω/m), Bessel |
|---:|---:|---:|
| 1e3 | 0.479 | 5.49409079963e-3 |
| 1e4 | 1.51 | 6.03978368192e-3 |
| 1e5 | 4.79 | 1.46073104771e-2 |
| 1e6 | 15.1 | 4.29286576531e-2 |

The check `r_1mhz` is R at 1 MHz. The DC resistance 1 / (σ π a²) is 5.48810148593e-3 Ω/m, so the skin effect multiplies R by 7.8. The square box makes the field outside the wire not quite round, but the first term that is not round is in (a / 10 mm)^4 = 1e-4 of the field at the wire, and it changes R much less than that.

## Mesh study

The solver sets the length of the outline segments of each conductor to a quarter of the skin depth at the highest frequency (16.5 µm at 1 MHz), and the mesh grows away from the outline. `max_cell` sets the size in the air and in the middle of the wire.

| max_cell (m) | points | R at 1 MHz (Ω/m) | error |
|---:|---:|---:|---:|
| 2e-3 | 5,063 | 4.3011707427e-2 | 1.93e-3 |
| 1e-3 | 5,569 | 4.3011707602e-2 | 1.93e-3 |
| 5e-4 | 7,906 | 4.3011708026e-2 | 1.93e-3 |

Thus the error comes from the length of the outline segments. With segments of 1, 1/2, 0.35 and 1/4 skin depth (the constant `SKIN` of `fh-fem`), the error is 4.4e-2, 1.1e-2, 4.6e-3 and 1.9e-3: it falls as the square of the length. The errors at 1 kHz and 100 kHz are below 1e-4 (the test `the_resistance_of_a_round_wire_follows_the_bessel_function_answer` of `crates/fem/tests/line.rs`).

## Tolerance

The check uses max_cell 1e-3 m. Its error is 1.9e-3 of the value. The tolerance is 4e-3.
