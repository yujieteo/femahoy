# grid-layered-wall derivation

This case checks the steady temperatures in a wall of 3 layers with a convection on each side (units SI-m). It is not a model of a product: it is the composite wall of a building.

## Source

F. P. Incropera, D. P. DeWitt, T. L. Bergman and A. S. Lavine, *Fundamentals of Heat and Mass Transfer*, 6th ed., Wiley, 2007, section 3.1.2 (the composite wall and its thermal circuit).
The resistances are in series: R = 1/h_in + Σ L_i/k_i + 1/h_out, and q = (T_in − T_out) / R.

## Model

| Item | Value |
|---|---|
| Plaster | x 0 to 0.015 m, k = 0.5 W/(m K) |
| Insulation | x 0.015 to 0.115 m, k = 0.04 W/(m K) |
| Brick | x 0.115 to 0.315 m, k = 0.8 W/(m K) |
| Inside, the face x− | h = 8 W/(m2 K), air at 20 °C |
| Outside, the face x+ | h = 25 W/(m2 K), air at −5 °C |
| Other faces | adiabatic, so the heat flows along x only |

R = 0.125 + 0.03 + 2.5 + 0.25 + 0.04 = 2.945 m2 K/W, and q = 25 / 2.945 = 8.488964 W/m2.

| Probe | Formula | T (°C) |
|---|---|---:|
| `surface_in` | 20 − q/8 | 18.938879457 |
| `plaster_insulation` | 20 − q (1/8 + 0.015/0.5) | 18.684210526 |
| `insulation_brick` | −5 + q (1/25 + 0.2/0.8) | −2.538200340 |
| `surface_out` | −5 + q/25 | −4.660441426 |

## Discretization error and tolerance

The exact temperature is linear in each layer, and a hex8 cell holds a linear field exactly. Thus the grid has no discretization error. The grids of max_cell 0.05 m and 0.025 m gave the same 4 values to 1e-12 °C. The tolerance 1e-6 °C covers only the round-off of the direct solve and of the reference values.
