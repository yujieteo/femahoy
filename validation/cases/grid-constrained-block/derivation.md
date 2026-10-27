# grid-constrained-block derivation

This case checks the orthotropic stiffness and the thermal strain of the grid solver. An orthotropic block cannot expand: rollers hold each face in its normal direction. The exact strain is 0 at each point, so the stress is exact on any grid (units SI-mm).

## Model

| Item | Value |
|---|---|
| Block | x 0 to 10 mm, y 0 to 8 mm, z 0 to 6 mm |
| Young's moduli | Ex = 100000, Ey = 50000, Ez = 20000 MPa |
| Poisson ratios | νxy = 0.3, νxz = 0.25, νyz = 0.2 |
| Shear moduli | Gxy = 20000, Gxz = 10000, Gyz = 8000 MPa |
| Expansion | αx = 5e-6, αy = 10e-6, αz = 20e-6 1/°C |
| Faces x− and x+ | ux = 0 |
| Faces y− and y+ | uy = 0 |
| Faces z− and z+ | uz = 0 |
| Temperature | 120 °C, stress-free at 20 °C, so ΔT = 100 °C |

## Reference

The total strain ε = S σ + α ΔT is 0, with the compliance S of the orthotropic material (R. M. Jones, Mechanics of Composite Materials, 2nd ed., Taylor and Francis 1999, section 2.5):

| | σxx | σyy | σzz |
|---|---|---|---|
| εxx | 1/Ex | −νxy/Ex | −νxz/Ex |
| εyy | −νxy/Ex | 1/Ey | −νyz/Ey |
| εzz | −νxz/Ex | −νyz/Ey | 1/Ez |

The solution of S σ = −α ΔT is σxx = −84.302009777, σyy = −72.650733297 and σzz = −50.027159153 MPa. The shear stresses and the displacements are 0.

## Tolerance

Each 8-node hex holds a uniform strain exactly, so the grid has no discretization error. The probe `centre` is inside the block, and the probe `corner` is on 3 faces with rollers. The direct solve gives the stress to about 1e-12 MPa. The tolerance 1e-6 MPa has a margin of 1e6.
