# grid-cantilever derivation

This case checks the bending of the 8-node hex with incompatible modes of the grid solver. A plain 8-node hex locks in bending and gives a beam that is too stiff. The reference is the beam theory (units SI-mm).

## Model

| Item | Value |
|---|---|
| Beam | x 0 to L = 100 mm, y 0 to b = 10 mm, z 0 to h = 10 mm |
| Material | E = 210000 MPa, ν = 0.3 |
| Root, x = 0 | ux = uy = uz = 0 |
| Tip, x = L | traction −1 MPa in z, so P = 100 N |

## Reference

I = b h³ / 12 = 833.333 mm⁴, A = b h = 100 mm², G = E / (2 (1 + ν)) = 80769.2 MPa.

The tip deflection of the Timoshenko beam is the bending part plus the shear part:

δ = P L³ / (3 E I) + P L / (κ G A) = 0.190476 + 0.001457 = 0.191933 mm,

with the shear coefficient of a rectangle κ = 10 (1 + ν) / (12 + 11 ν) = 0.8497 (G. R. Cowper, The shear coefficient in Timoshenko's beam theory, J. Appl. Mech. 33 (1966) 335-340). The probe `tip` is at the centre of the tip face, so `tip.uz` = −0.191933 mm.

The bending stress is σxx = M z / I, with M = P (L − x) and z from the mid-plane. This is the exact Saint-Venant solution of a beam with an end shear force, away from the root and the tip (S. P. Timoshenko and J. N. Goodier, Theory of Elasticity, 3rd ed., McGraw-Hill 1970, section 120). At x = 51.25 mm, z = ±5 mm: σxx = ±100 · 48.75 · 5 / 833.333 = ±29.25 MPa.

## Mesh study

| max_cell (mm) | unknowns | tip.uz (mm) | middle_top.sxx (MPa) |
|---:|---:|---:|---:|
| 5 | 540 | −0.189395 | 28.435 |
| 2.5 | 3,000 | −0.190113 | 29.250 |
| 1.25 | 19,440 | −0.190444 | 29.633 |
| 0.625 | 138,720 | −0.190579 | 29.440 |

The changes of `tip.uz` fall by a factor of about 2.4 for each halving, and the extrapolation gives −0.190671 mm. This is 0.66 % less than the beam theory: the clamp of all 3 components holds the Poisson contraction and the warping of the root section, which the beam theory does not. The error at max_cell 2.5 mm is 0.30 %.

The stress probes are at the centre of a cell in x only at max_cell 2.5 mm. There, the grid gives 29.2500000 MPa. The incompatible modes make the cell exact for a linear moment at its centre. At the other grids, the probes are on a cell face, and the stress is the value of 1 cell at its edge.

## Tolerance

The sum of the 0.66 % of the clamp and the 0.30 % of the discretization is 0.96 %. The tolerance is 1.5 %.
