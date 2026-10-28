# grid-holm-spot derivation

This case checks a conduction step on the mesh of tetrahedra with a singular field: the constriction resistance of a circular contact spot. It is not a package problem. Units are SI-mm.

## Source

A circular spot of radius a at 1 potential on the face of a half-space of conductivity σ, with the other electrode at infinity, has the constriction resistance R0 = 1 / (4 σ a). R. Holm, *Electric Contacts*, Springer, 1967, https://doi.org/10.1007/978-3-662-06688-1: the constriction resistance of a circular spot. The exact potential has oblate spheroidal equipotentials: in the coordinates (ξ, η) with r = a cosh ξ sin η and depth a sinh ξ cos η, V = V0 (1 − (2/π) atan(sinh ξ)) and the current is 4 σ a V0. Thus the resistance between the spot and the spheroid ξ is R(ξ) = R0 (2/π) atan(sinh ξ).

## Bounds for the finite block

The model is a round block of radius b and height b, with the spot at the centre of the top face, and the bottom and the round side at 0 V. A domain with its boundary at 0 V that holds another such domain has the larger resistance (Rayleigh's monotonicity: the added material is in series). The block holds the half-spheroid with the equatorial radius a cosh ξ1 = b (its depth a sinh ξ1 < b), and the half-spheroid with sinh ξ2 = √2 b / a holds the block. Thus R(ξ1) ≤ R ≤ R(ξ2).

| Item | Value |
|---|---|
| Spot | a = 1 mm, 1 V |
| Block | b = 20 mm, height 20 mm, copper σ = 5.8e7 S/m; bottom and round side at 0 V |
| R0 = 1 / (4 σ a) | 4.3103448e-6 ohm |
| R(ξ1) / R0, sinh ξ1 = √399 | 0.968156 |
| R(ξ2) / R0, sinh ξ2 = 28.284 | 0.977501 |

The reference is the middle of the bounds, 0.972829 R0 = 4.1932267e-6 ohm, with a half-width of 0.48 %.

## Discretization error and tolerance

With fixed voltages, the finite element solution has the smallest energy of its space, so its conductance is too large and its resistance too small: the result is below the exact value. The current density is singular at the rim of the spot (as 1 / sqrt(distance)), so the error falls slowly with the cell. The parts `inner` (radius 0.98 mm), `ring` (1.02 mm) and the thin part `spot` (0.02 mm deep) put small cells at the rim. The 64 segments of the polygon of the spot make its area 0.16 % smaller, which adds about 0.08 % to R.

| Mesh at the rim | points | R / R0 | time |
|---|---:|---:|---:|
| no rings, spot 0.2 mm deep, max_cell 2 | 39,474 | 0.92959 | 2.5 s |
| no rings, spot 0.05 mm deep, max_cell 2 | 52,632 | 0.94164 | 3.6 s |
| rings 0.95 and 1.05 mm, spot 0.05 mm deep, max_cell 2 | 57,492 | 0.94936 | 10.6 s |
| rings 0.98 and 1.02 mm, spot 0.02 mm deep, max_cell 4 (the check) | 64,008 | 0.95448 | 7.1 s |
| rings 0.99 and 1.01 mm, spot 0.01 mm deep, max_cell 4 | 130,942 | 0.95921 | 24 s |

Each finer rim moves R up towards the bounds, and no run is above the lower bound 0.968156. The check gives 0.95448 R0: 1.4 % below the lower bound and 2.4 % below the upper bound, so its error is between −1.4 % and −2.4 %. The tolerance 3 % holds the largest error with a margin of 0.6 %.
