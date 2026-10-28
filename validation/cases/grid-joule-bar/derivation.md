# grid-joule-bar derivation

This case checks the Joule heat of a conduction step: the electric solve gives the Joule power density, and a steady heat solve on the same grid takes it as its heat source. It is not a package problem. Units are SI-m.

## Source

A plane wall with a uniform heat generation q, a conductivity k, a thickness L and the temperature T0 on both faces has the parabolic temperature T(x) = T0 + q x (L − x) / (2 k), with its peak T0 + q L² / (8 k) at the middle. F. P. Incropera, D. P. DeWitt, T. L. Bergman and A. S. Lavine, *Fundamentals of Heat and Mass Transfer*, 6th ed., Wiley, 2007, section 3.5. A uniform bar with the voltage V between its ends has a uniform current density, and q = σ (V / L)².

## Model

| Item | Value |
|---|---|
| Bar | x 0 to 0.2 m, y and z 0 to 0.01 m |
| Steel | σ = 1e6 S/m, k = 50 W/(m K) |
| Ends | 0.01 V on x−, 0 V on x+, and 20 °C on both |
| Long sides | adiabatic and with no current |
| Grid | max_cell 0.01 m: 20 cells along x |
| Probe | `middle` at x = 0.1 m, a point of the grid |

q = 1e6 × (0.01 / 0.2)² = 2500 W/m³. T_max = 20 + 2500 × 0.04 / 400 = 20.25 °C. The current is I = σ A V / L = 1e6 × 1e-4 × 0.01 / 0.2 = 5 A.

| Check | Formula | Value |
|---|---|---|
| `peak` | T0 + q L² / (8 k) | 20.25 °C |
| `current` | σ A V / L | 5 A |

## Error and tolerance

The field is 1D in x. The trilinear cells hold the linear potential exactly. The lumped Joule heat of each cell (an equal share to each of its points) is the consistent load of a uniform source on a linear element, and for this load, linear elements give the exact temperature at the points (Hughes, *The Finite Element Method*, chapter 1). The run gave 20.25 °C and 5 A, with all printed digits correct. The tolerance 1e-9 holds the round-off of the direct solves.
