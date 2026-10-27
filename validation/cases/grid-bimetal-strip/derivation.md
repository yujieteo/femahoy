# grid-bimetal-strip derivation

This case checks the thermal strain of 2 bonded materials, the bending of thin layers, and the curvature of a surface. A free strip of 2 layers with different expansion coefficients is heated. It bends to a uniform curvature (units SI-mm).

## Model

| Item | Value |
|---|---|
| Strip | x −50 to 50 mm, y −5 to 5 mm |
| Layer 1, bottom | z 0 to 0.5 mm, E1 = 200000 MPa, ν = 0.3, α1 = 12e-6 1/°C |
| Layer 2, top | z 0.5 to 1 mm, E2 = 100000 MPa, ν = 0.3, α2 = 20e-6 1/°C |
| Temperature | 120 °C, stress-free at 20 °C, so ΔT = 100 °C |
| Support | uz = 0 on the bottom faces at the centre; the planes x = 0 and y = 0 are planes of symmetry |

The load has no net force, so the support only stops a rigid motion.

## Reference

S. P. Timoshenko, Analysis of bi-metal thermostats, J. Opt. Soc. Am. 11 (1925) 233-255, gives the curvature of a strip of 2 layers:

κ = 6 (α2 − α1) ΔT (1 + m)² / (h [3 (1 + m)² + (1 + m n) (m² + 1 / (m n))]),

with m = t1 / t2 = 1, n = E1 / E2 = 2 and h = t1 + t2 = 1 mm:

κ = 6 · 8e-6 · 100 · 4 / (1 · [12 + 3 · 1.5]) = 0.0192 / 16.5 = 1.163636e-3 1/mm.

The layer with the larger expansion is on the top, so the top face bulges outward (`top.shape` is `convex`), and the curvature of the top face is positive.

The strip is 10 times as wide as it is thick, so it bends in x and in y. The 2 layers have the same Poisson ratio. Then the in-plane stress is equal in x and y, and the formula holds for each direction with the biaxial moduli E / (1 − ν). The formula uses only the ratio n of the moduli, so the curvature is the same in x and in y.

## Mesh study

Each layer has 2 cells in z at each grid (`min_cells = 2`).

| max_cell (mm) | unknowns | top.kxx (1/mm) | top.kyy (1/mm) |
|---:|---:|---:|---:|
| 2 | 1,406 | 1.122349e-3 | 1.132433e-3 |
| 1 | 4,301 | 1.158686e-3 | 1.157552e-3 |
| 0.5 | 16,096 | 1.158975e-3 | 1.157798e-3 |
| 0.25 | 62,180 | 1.159359e-3 | 1.158216e-3 |

## Tolerance

The surface curvature is the least squares fit of a quadratic to the displacement of the whole top face. Near the free edges, in a zone about 1 thickness wide, the bending is not uniform, and the fit includes these zones. Thus the finest grid is 0.37 % (kxx) and 0.47 % (kyy) below the theory. The change from max_cell 1 mm to 0.25 mm is 0.06 %, so the discretization error at 1 mm is about 0.06 %. The sum is 0.53 %, and the tolerance is 1 %.
