# grid-waveguide-dielectric derivation

This case checks the built-in electromagnetic solver at a change of material: a PTFE section in a rectangular waveguide. The section reflects a part of the wave at its 2 faces. The case is not a chip package problem. Units are SI-mm.

## Source

In a guide of uniform cross-section, a section that fills the cross-section keeps the TE10 shape: the wave sees only a change of the propagation constant. The section is thus a transmission line of length l with the TE10 wave impedance Z2 = ωμ0 / β2, between 2 lines of Z1 = ωμ0 / β1, with βi = sqrt(εi k0² − (π/a)²). The answer is exact for the TE10 wave (no other mode is excited). D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley, 2012, sections 3.3 (TE10 in a filled guide), 4.4 (the ABCD matrix of a line) and 4.4 (the conversion of ABCD to S).

The ABCD matrix of the section is A = D = cos β2l, B = j Z2 sin β2l, C = j sin β2l / Z2. With the reference impedance Z1: S11 = (A + B/Z1 − C Z1 − D) / Δ and S21 = 2 / Δ, with Δ = A + B/Z1 + C Z1 + D. Each port is 10 mm from the section, so each S-parameter gets the factor exp(−2jβ1 × 10 mm).

## Model

| Item | Value |
|---|---|
| Guide | WR-90, a = 22.86 mm, b = 10.16 mm, z = 0 to 30 mm, air |
| Section | PTFE εr = 2.2, z = 10 to 20 mm, the full cross-section |
| Walls | perfect conductors (the default wall) |
| Ports | TE10 at z = 0 (port 1) and z = 30 mm (port 2) |
| Frequency | 10 GHz |

β1 = 158.238256 rad/m and β2 = 278.837125 rad/m.

| Check | Value |
|---|---|
| `s11_re`, `s11_im` | 0.075518 − 0.187732 j (\|S11\| = 0.202352) |
| `s21_re`, `s21_im` | 0.908557 + 0.365483 j (\|S21\| = 0.979313) |

## Mesh study

The table gives the error of each part.

| max_cell (mm) | points | unknowns | S11 re | S11 im | S21 re | S21 im | time |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 3 | 949 | 4,106 | −0.00725 | 0.01039 | 0.00719 | −0.01260 | 0.03 s |
| 1.5 | 5,500 | 30,416 | −0.00303 | 0.00367 | 0.00347 | −0.00636 | 1.1 s |
| 0.75 | 38,700 | 240,545 | −0.00075 | 0.00089 | 0.00084 | −0.00150 | 80 s |

The errors fall by a factor of 2 to 3 and then 4 when the cell halves: second order.

## Tolerance

The check uses max_cell 1.5 mm. Its largest error is 0.0064 (S21 im). The pass rule uses abs_tol 0.012 for each part.
