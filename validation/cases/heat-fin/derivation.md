# heat-fin derivation

This case checks the steady tip temperature of an aluminium pin fin (units SI-mm).

## Source

F. P. Incropera, D. P. DeWitt, T. L. Bergman and A. S. Lavine, *Fundamentals of Heat and Mass Transfer*, 6th ed., Wiley, 2007, Table 3.4, case A (convection heat transfer at the tip).
At the tip, θ(L) / θb = 1 / (cosh mL + (h / mk) sinh mL), with θ = T - T∞ and m = sqrt(h P / (k A)).

## Model

| Item | Value |
|---|---|
| Fin | 100 mm in x, 10 x 10 mm section |
| Aluminium | k = 200 W/(m K), which is 200 mW/(mm K) in SI-mm |
| Base | `BASE`, the face x = 0: T = 100 °C |
| Convection | the 4 sides and the tip: h = 50 W/(m2 K) = 0.05 mW/(mm2 K), T∞ = 20 °C |
| Step | `*HEAT TRANSFER, STEADY STATE` |

P = 0.04 m and A = 1e-4 m2. Thus m = sqrt(50 · 0.04 / (200 · 1e-4)) = 10 1/m, mL = 1 and h / mk = 0.025.
T_tip = 20 + 80 / (cosh 1 + 0.025 sinh 1) = 70.8757 °C.

`*FILM` in ccx 2.23 refuses a surface name. Thus `job.inp` has 1 line `E, Fn, 20.0, 0.05` for each element face of the surfaces `TIP`, `Y0`, `Y1`, `Z0` and `Z1`.

## Tolerance

The tolerance is 0.1 °C (`abs_tol = 0.1`), 0.2 % of the 50.9 °C difference between the tip and the air.
The fin theory has 1 temperature in each section. In the solid, the tip face changes from 70.84 °C at the corners to 70.90 °C at the centre: the Biot number h (t/2) / k is 0.00125.
Thus the check is the mean of the tip face. The mean is 70.8776 °C (+0.002 °C) with a mesh of size 5 mm.

## Chat run

The request asked for the temperature at the centre of the tip face in plain words.
The chat used a mesh of size 2.5 mm (6695 nodes). It gave 70.904 °C (+0.029 °C) at the nodes near the centre of the tip.
The run took 111 s and cost 0.50 USD.

The chat run found 2 gaps:

1. The first solve stopped: ccx refused `*FILM` with the surface name (`TIP, F, 20.0, 0.05`). The chat wrote a script that lists the element faces. `crates/fh/src/daemon/agent.md` now has this fact.
2. The 3D view showed no temperature, because the reader of the `.frd` file kept the name `NDTEMP`. The reader now gives `NT`, and the view shows `NT` first when the step has no stress and no displacement.

## Files

The mesh in `job.inp` comes from `fh mesh part.step --size 5 --face BASE=x:0 --face TIP=x:100 --face Y0=y:0 --face Y1=y:10 --face Z0=z:0 --face Z1=z:10`.
`part.step` is a box of 100 x 10 x 10 mm with a corner at the origin.
