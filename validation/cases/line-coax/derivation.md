# line-coax derivation

This case checks the transmission line solver of the built-in grid solver (`fh guide grid`, a `grid.toml` with a `[line]` table) on a coaxial cable with an exact answer. The case is not a chip package problem. Units are SI-m.

## Source

The field between 2 coaxial perfect conductors is the field of a line charge and of a line current. With the inner radius a, the outer radius b and a dielectric of relative permittivity ε_r: C = 2π ε0 ε_r / ln(b/a), L = μ0 ln(b/a) / (2π), Z0 = sqrt(L / C) and ε_eff = L C c0² = ε_r. D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley, 2012, section 2.2 (the coaxial line). The constants are ε0 = 8.8541878128e-12 F/m, μ0 = 1.25663706212e-6 H/m (CODATA 2018) and c0 = 299792458 m/s. The values below are from mpmath at 30 digits.

## Model

| Item | Value |
|---|---|
| Inner conductor | perfect conductor, circle of radius a = 0.5 mm, the signal |
| Dielectric | ε_r = 2.25, no loss, between a and b = 1.75 mm |
| Outer conductor | perfect conductor, tube from b to 2.0 mm, the reference |
| Walls | none (the field is zero outside the outer conductor) |
| Frequency | 1 GHz (a line of perfect conductors has the same L at each frequency) |

| Check | Formula | Value |
|---|---|---|
| `c` | 2π ε0 ε_r / ln(b/a) | 9.99176495361e-11 F/m |
| `l` | μ0 ln(b/a) / (2π) | 2.50552593835e-7 H/m |
| `z0` | sqrt(L / C) | 50.0758519761 Ω |
| `eps_eff` | ε_r | 2.25 |

## Mesh study

The points of each circle are on the circle, with segments of at most `max_cell`. Thus the conductors are polygons: the inner one is a little too small and the outer one a little too large, so C is too small and L too large. These errors and the error of the linear triangles fall at second order.

| max_cell (m) | points | C (F/m) | L (H/m) | Z0 (Ω) | time |
|---:|---:|---:|---:|---:|---:|
| 8e-5 | 4,091 | 9.98778e-11 | 2.50653e-7 | 50.0958 | 0.01 s |
| 4e-5 | 16,278 | 9.99065e-11 | 2.50581e-7 | 50.0814 | 0.06 s |
| 2e-5 | 64,493 | 9.99148e-11 | 2.50560e-7 | 50.0773 | 0.29 s |

The error of L is 4.0e-4, 1.1e-4 and 3.0e-5 of the value: it falls by about 4 when the cell halves. The effective permittivity is 2.25 to 10 digits in each mesh, because in 1 dielectric the electrostatic and the magnetic solve use the same matrix.

## Tolerance

The check uses max_cell 4e-5 m. Its largest relative error is 2.2e-4 (L). The tolerance is 5e-4 of each value.
