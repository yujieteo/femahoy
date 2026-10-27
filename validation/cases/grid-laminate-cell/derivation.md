# grid-laminate-cell derivation

This case checks the homogenized material of a periodic unit cell with the exact answer of a laminate: the conductivity, the stiffness, the expansion, the density and the specific heat. The cell is 2 bonded layers of aluminium and a polymer along z (units SI-mm).

## Model

| Item | Value |
|---|---|
| Cell | 1 x 1 x 1 mm |
| Layer 1, aluminium | z 0 to 0.3 mm (f1 = 0.3), k = 200, E = 70000 MPa, ν = 0.33, α = 23e-6 1/°C, ρ = 2.7e-9, c = 9.0e8 |
| Layer 2, polymer | z 0.3 to 1 mm (f2 = 0.7), k = 0.25, E = 3000 MPa, ν = 0.38, α = 70e-6 1/°C, ρ = 1.2e-9, c = 1.5e9 |
| Step | `type = "homogenize"`, periodic on the 6 sides of the cell |

## Reference

The mean over the 2 layers is ⟨g⟩ = f1 g1 + f2 g2. In a laminate, the in-plane strains and the stresses across the layers (σzz, σyz, σxz) are the same in each layer. The other fields follow from them in each layer (R. M. Jones, Mechanics of Composite Materials, 2nd ed., Taylor and Francis 1999, chapter 4; the bounds of W. Voigt 1889 and A. Reuss 1929 are exact for each direction):

- Conductivity: kxx = kyy = ⟨k⟩ = 60.175 (the Voigt mean along the layers), and kzz = 1 / ⟨1/k⟩ = 0.3569516331 (the Reuss mean across them).
- Stiffness, with λ and G the Lamé constants of each layer and C11 = λ + 2G: C33* = 1 / ⟨1/C11⟩ = 7840.818918, C13* = ⟨λ/C11⟩ C33* = 4522.533011, C11* = ⟨C11 − λ²/C11⟩ + ⟨λ/C11⟩² C33* = 28629.36401, C12* = ⟨λ − λ²/C11⟩ + ⟨λ/C11⟩² C33* = 11318.1512, C44* = C55* = 1 / ⟨1/G⟩ = 1525.78578, C66* = ⟨G⟩ = 8655.606407 (MPa).
- Expansion: with no mean stress, the in-plane strain ε is the same in the 2 layers and the in-plane forces cancel. With B = E / (1 − ν), αxx = ⟨B α⟩ / ⟨B⟩ = 2.758369723e-5. Across the layers σzz = 0, so εzz = α − 2ν/(1 − ν) (ε − α) in each layer, and αzz = ⟨α (1 + ν)/(1 − ν)⟩ − αxx ⟨2ν/(1 − ν)⟩ = 9.094133916e-5.
- Density ⟨ρ⟩ = 1.65e-9 and specific heat ⟨ρ c⟩ / ⟨ρ⟩ = 1.205454545e9.

The script [laminate.py](laminate.py) computes these numbers.

## Mesh study

| max_cell (mm) | kzz | c33 (MPa) | alphazz (1/°C) |
|---:|---:|---:|---:|
| 0.25 | 0.35695163305 | 7840.8189182 | 9.0941339156e-5 |

The values are the reference to 10 digits. In each layer the exact fluctuation of each load case is linear in z, and the cells hold a linear field exactly, so a finer grid gives the same values.

## Tolerance

The discretization error is 0. The tolerance 1e-6 is the precision of the direct solve and of the 10 digits of `homogenized.txt`, with a large margin.
