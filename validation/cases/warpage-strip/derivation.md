# Bilayer warpage through the CAD workflow

The reference is the bilayer solution in Lubarda and Lubarda, section 2, equations 2.5 to 2.7.
Source: [the authors’ public paper](http://maeresearch.ucsd.edu/~vlubarda/research/pdfpapers/JTS-22A.pdf).
The title is *On the curvature and internal stresses in a multilayer strip due to uniform heating, electric field, or hydration*.
The paper gives an analytical result, independent of CalculiX.
The dimensions and material values below are selected inputs to that result, not published measurements.

The full strip is 80 x 4 x 2 mm. Each bonded layer is 1 mm thick.
The quarter model uses x = 0 to 40, y = 0 to 2, z = 0 to 2 mm.

| Layer | z (mm) | E (MPa) | nu | alpha (1/K) |
|---|---|---:|---:|---:|
| LOW | 0 to 1 | 200000 | 0 | 1.2e-5 |
| HIGH | 1 to 2 | 70000 | 0 | 2.3e-5 |

The uniform temperature change is 100 K, from a stress-free value of 0 to 100.
Both Poisson ratios are 0 to remove transverse coupling from the beam reference.
Symmetry gives u1 = 0 at x = 0 and u2 = 0 at y = 0.
The origin has u3 = 0 to remove rigid translation.

With c_i = E_i b_i h_i, c = c_1 + c_2 and k = (c_1 h_1^2 + c_2 h_2^2)/12,
the curvature magnitude is 2 c_1 c_2 (h_1 + h_2) (alpha_HIGH - alpha_LOW) dT / (4 c k + c_1 c_2 (h_1 + h_2)^2).
This gives kappa = 7/9125 = 0.000767123287671233 1/mm.
The mean tip displacement is -kappa x^2/2 = -0.6136986301369863 mm at x = 40 mm.
The high-expansion upper layer gives the negative z displacement.
The reference neglects end effects and through-thickness displacement offsets.

## Tolerance

The relative tolerance is 0.01. It covers tetrahedral discretization, tip effects and the displacement offset through the thickness.
The tolerance is larger than the tolerance of `bimetal-strip`, which checks a regular C3D8I mesh away from the tip.
This case checks the complete workflow with an unstructured C3D10 mesh.

## Reproduce

Run `fh-python scripts/check-warpage.py /tmp/warpage-proof`. It makes the STEP file with build123d and checks 2 mesh sizes.
The script uses a private daemon state folder and the CalculiX adapter.

## Measured result

The check passed on macOS arm64 with Python 3.12.13, build123d 0.13.0, gmsh 4.15.2-git and CalculiX 2.23.
The solver time is `wall_s` from the adapter summary. It includes the solve and adapter work before result output.

| Mesh size (mm) | Nodes | C3D10 elements | Shared interface nodes | Warpage (mm) | Reference (mm) | Absolute difference (mm) | Difference (%) | Solver time (s) |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1.0 | 3216 | 1623 | 417 | -0.6126390244 | -0.6136986301 | 0.0010596057 | 0.172659 | 0.3825 |
| 0.5 | 13720 | 7867 | 1849 | -0.6126606000 | -0.6136986301 | 0.0010380301 | 0.169143 | 1.349 |

Both differences are below the 1% tolerance. The two computed values differ by 0.00352% of the reference.
The small residual includes the three-dimensional free-end effect in the solid model.
The committed `strip.inp` is the coarse mesh from `fh mesh --size 1` and `fh model`.
The STEP file comes from `scripts/cad/warpage_strip.py`. gmsh fragments the touching bodies to share interface nodes.
Each solid element belongs to exactly one material group.
The workflow check also reads the VTU fields and refuses this input through the native solver.

Run proof: `/Users/yj/src/firstmate/data/femahoy-warpage-case-w1/proof/record.json` and the adjacent command logs.
The source record is `/Users/yj/src/firstmate/data/femahoy-warpage-case-w1/source.txt`.

The browser check used the built wasm viewer and the fine mesh from `fh run`.
The viewer showed 13720 nodes, 7867 elements, SI-mm units, MISES contours and the deformed strip.
The largest displacement was 0.6193 mm. This is the vector magnitude, not the mean tip warpage.
The screenshot is `/Users/yj/src/firstmate/data/femahoy-warpage-case-w1/viewer.png`.
