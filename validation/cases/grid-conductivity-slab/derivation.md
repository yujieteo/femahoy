# grid-conductivity-slab derivation

This case checks a conductivity that changes with the temperature against an exact answer. The case is not a chip package problem. Units are SI-m, and the temperatures are in kelvin.

## Source

With k = k0 (1 + beta T), the Kirchhoff transform Phi(T) = integral of k dT = k0 (T + beta T^2 / 2) changes the steady equation div(k grad T) = 0 into the Laplace equation of Phi, because grad Phi = k grad T. In a plane wall with no source (Incropera et al., *Fundamentals of Heat and Mass Transfer*, 6th ed., section 3.1), the flux k dT/dz = dPhi/dz is the same at each z, so Phi is linear in z. Thus

Phi(z) = Phi(T1) + (Phi(T2) − Phi(T1)) z / L, and T(z) = (−1 + sqrt(1 + 2 beta Phi(z) / k0)) / beta.

## Model

| Item | Value |
|---|---|
| Slab | x and y 0 to 0.05 m, z 0 to 1 m |
| Conductivity | k0 = 2 W/(m K), beta = 0.002 1/K: the table (300 K, 3.2) and (500 K, 4.0) holds it exactly between the 2 face temperatures |
| Faces | T1 = 500 K at z = 0, T2 = 300 K at z = 1 m |
| Grid | max_cell 0.05 m: 20 cells along z |

| Check | z (m) | Value |
|---|---:|---:|
| `quarter` | 0.25, a point of the grid | 453.939201417 K |
| `middle` | 0.5, a point of the grid | 405.538513814 K |
| `cell_middle` | 0.725, halfway between 2 points | 359.651092013 K |

## Error

The solver gives each cell the conductivity at the mean temperature of its points. In a slab, the flux of a cell is then k(T_m) (T_a − T_b) / h. When k is linear in T, this equals the Kirchhoff flux (Phi(T_a) − Phi(T_b)) / h, so the solution is exact at the points. The solve took 8 iterations, and `quarter` and `middle` equal the exact values to 1e-11. The iteration converges linearly, because its tangent has no dk/dT term (see `crates/fem/src/nonlinear.rs`).

Between 2 points, the result is the linear interpolation of the curved exact profile. Its error is h^2 / 8 |T''| = 4.4e-5 of the value at `cell_middle` (computed 359.635153, exact 359.651092). The tolerance 1e-4 holds it with a margin of a factor of 2. The budget of the method was 0.5 % at 20 cells: the case is 50 times tighter.
