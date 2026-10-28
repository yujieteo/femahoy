# grid-resistor-bar derivation

This case checks the DC conduction step of the built-in grid solver (`[step] type = "conduction"`): the resistance between 2 face sets with a fixed voltage, and the unit system SI-mm (lengths in mm, the electric conductivity in S/m, the resistance in ohm). It is not a package problem.

## Source

A uniform bar of length L, section A and conductivity σ, with 1 voltage on each end face, has a uniform current density and R = L / (σ A). The FE equation is the steady conduction equation of T. J. R. Hughes, *The Finite Element Method*, Prentice-Hall, 1987, chapter 1, with the potential in the place of the temperature.

## Model

| Item | Value |
|---|---|
| Bar | x 0 to 10 mm, y 0 to 1 mm, z 0 to 2 mm |
| Copper | σ = 5.8e7 S/m |
| Ends | 1 V on x−, 0 V on x+ |

R = 0.01 m / (5.8e7 S/m × 2e-6 m²) = 8.6206896552e-5 ohm.

## Error and tolerance

The exact potential is linear in x. The trilinear cells hold a linear field exactly, so the result has no discretization error. The run gave 8.6206896552e-5 ohm (all printed digits correct). The tolerance 1e-9 holds the round-off of the direct solve.
