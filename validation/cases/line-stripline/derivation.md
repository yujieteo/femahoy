# line-stripline derivation

This case checks the transmission line solver (`fh guide grid`, a `[line]` table) on a stripline against a published closed form. A stripline is a printed circuit board line, and the same cross-section is the TEM cell of a test laboratory. Units are SI-m.

## Source

A strip of width w and zero thickness in the middle between 2 infinite ground planes a distance b apart, in a dielectric of relative permittivity ε_r, has the exact characteristic impedance Z0 = (η0 / (4 sqrt(ε_r))) K(k) / K(k'), with k = sech(π w / (2b)), k' = tanh(π w / (2b)), η0 = sqrt(μ0 / ε0) and K the complete elliptic integral of the first kind. S. B. Cohn, Characteristic impedance of the shielded-strip transmission line, IRE Trans. Microwave Theory Tech. 2 (1954) 52-57. The formula is from a conformal map, so it has no approximation for a strip of zero thickness. Then C = sqrt(ε_r) / (c0 Z0), L = Z0 sqrt(ε_r) / c0 and ε_eff = ε_r. The values below are from mpmath at 30 digits.

## Model

| Item | Value |
|---|---|
| Ground planes | the electric walls y− and y+, b = 2 mm apart |
| Strip | perfect conductor, w = 1 mm, thickness t = 0.2 µm (1e-4 b), in the middle, the signal |
| Dielectric | ε_r = 2.2, no loss, x from −20 mm to 20 mm |
| Sides | magnetic walls at x = ±20 mm, 10 b from the edge of the strip |

| Check | Value |
|---|---|
| `z0` | 67.7115445059 Ω |
| `c` | 7.30681172969e-11 F/m |
| `l` | 3.35006595745e-7 H/m |
| `eps_eff` | 2.2 |

The field decays as e^(−π x / b) away from the strip, so the side walls change Z0 by about e^(−10π) = 2e-14.

## Mesh study

The field is singular at the edges of the strip. The mesh is fine at the edges (about the thickness of the strip) and grows with the factor `max_growth`. The error depends on that factor more than on `max_cell`.

| max_cell (m) | max_growth | points | Z0 (Ω) | error |
|---:|---:|---:|---:|---:|
| 4e-4 | 1.3 | 10,913 | 67.3309 | −5.6e-3 |
| 2e-4 | 1.3 | 14,153 | 67.4317 | −4.1e-3 |
| 1e-4 | 1.3 | 27,142 | 67.5437 | −2.5e-3 |
| 5e-5 | 1.3 | 78,082 | 67.6105 | −1.5e-3 |
| 2.5e-5 | 1.3 | 279,050 | 67.6449 | −9.8e-4 |
| 1e-4 | 1.1 | 34,758 | 67.6422 | −1.0e-3 |
| 5e-5 | 1.1 | 84,560 | 67.6567 | −8.1e-4 |
| 1e-4 | 1.05 | 59,543 | 67.6684 | −6.4e-4 |

A strip of thickness 0.02 µm (max_cell 1e-4 m, max_growth 1.1) gives 67.6705 Ω, 4.2e-4 above the strip of 0.2 µm. The effect of the thickness grows as t ln(1/t), so the strip of 0.2 µm is about 4.6e-4 below the strip of zero thickness. The rest of the error at max_growth 1.05, about 2e-4, is the mesh.

## Tolerance

The check uses max_cell 1e-4 m and max_growth 1.05. Its error is 6.4e-4 of Z0, C and L. The tolerance is 1e-3. For an impedance to 0.1 %, use max_growth 1.1 or less, because the default 1.3 gives a mesh that is too coarse at the edges of a thin strip.
