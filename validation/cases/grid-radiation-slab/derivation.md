# grid-radiation-slab derivation

This case checks the radiation to the ambient of the built-in grid solver against an exact answer. A slab has a fixed temperature on 1 face, and the other face radiates to large surroundings. The case is not a chip package problem. Units are SI-m, and the temperatures are in kelvin.

## Source

A face that is small against its surroundings at T_amb loses q = eps sigma (T^4 − T_amb^4) per area (F. P. Incropera, D. P. DeWitt, T. L. Bergman and A. S. Lavine, *Fundamentals of Heat and Mass Transfer*, 6th ed., Wiley, 2007, section 1.2.3, radiation exchange with large surroundings). The slab has no source, so its temperature is linear in z, and the conduction through it equals the radiation from its face:

k (T1 − T2) / L = eps sigma (T2^4 − T_amb^4).

The face temperature T2 is the root of this scalar equation in (T_amb, T1). Bisection to the last bit gives it.

## Model

| Item | Value |
|---|---|
| Slab | k = 10 W/(m K), x and y 0 to 0.01 m, z 0 to 0.1 m |
| Face z = 0 | T1 = 1000 K |
| Face z = L | radiation, eps = 0.8, T_amb = 300 K, sigma = 5.670374419e-8 W/(m2 K4) |
| Other faces | adiabatic |
| Grid | max_cell 0.01 m: 10 cells along z |

| Check | Formula | Value |
|---|---|---|
| `face`, z = L | the root T2 | 809.185667706 K |
| `middle`, z = L / 2 | (T1 + T2) / 2 | 904.592833853 K |
| `radiation`, the heat into the body at the face | −k (T1 − T2) / L · 1e-4 m2 | −1.908143323 W |

## Error

The exact field is linear in z, and the linear cells hold a linear field exactly. The face has 1 temperature, so the lumped radiation of its points is exact. Thus the only errors are the stop of the Newton iteration (a change below 1e-10 of the largest temperature) and the direct solve. The solve took 6 Newton iterations from the start at T_amb, and its results equal the exact values to all 11 printed digits. The tolerance is 1e-8 of each value.
