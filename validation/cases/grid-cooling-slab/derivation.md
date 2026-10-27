# grid-cooling-slab derivation

This case checks the transient centre temperature of a slab that cools after its 2 faces go to 0 °C (units SI-m). It also measures the error of the time steps and the cells.

## Source

H. S. Carslaw and J. C. Jaeger, *Conduction of Heat in Solids*, 2nd ed., Oxford, 1959, section 3.3 (the slab −L < z < L at T0, with its faces at 0 from t = 0).
At the centre, T / T0 = Σ_{n ≥ 0} 4 (−1)^n / ((2n+1)π) · exp(−(2n+1)² π² Fo / 4), with Fo = α t / L².

## Model

| Item | Value |
|---|---|
| Slab | z 0 to 0.02 m (L = 0.01 m), x and y 0 to 0.001 m |
| Material | k = 1 W/(m K), ρ = 1000 kg/m3, c = 1000 J/(kg K), so α = 1e-6 m2/s |
| Faces z− and z+ | T = 0 °C from t = 0 |
| Other faces | adiabatic, so the heat flows along z only |
| Start | T0 = 100 °C |
| Step | transient, backward Euler, dt = 0.1 s, end 20 s |
| Symmetry | z: the solver solves 1 half |

At t = 20 s, Fo = 0.2. 50 terms of the series give T(centre) = 77.231160686 °C.

## Discretization error and tolerance

T(centre) at t = 20 s, in °C:

| max_cell (m) \ dt (s) | 0.2 | 0.1 | 0.05 | 0.025 |
|---:|---:|---:|---:|---:|
| 0.001 | 77.16018 | 77.13918 | 77.12847 | 77.12306 |
| 0.0005 | 77.24674 | 77.22512 | 77.21410 | 77.20853 |
| 0.00025 | 77.26825 | 77.24647 | 77.23536 | 77.22976 |

Each column changes by a factor of about 4 when the cell halves (second order in space). Each row changes by a factor of about 2 when dt halves (first order in time, backward Euler).
At max_cell 0.0005 m and dt = 0.1 s, the space error is about −0.029 °C and the time error is about +0.022 °C. Their magnitudes add to 0.051 °C. The tolerance is 0.06 °C. The case result is 77.2251 °C, 0.006 °C from the exact value, because the 2 errors have opposite signs.
