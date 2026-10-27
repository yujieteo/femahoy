# grid-round-cantilever derivation

This case checks the bending of the 10-node tetrahedra on a bar of round section along the x axis. The bar is a `circle` part with `axis = "x"`, so the solver meshes it with tetrahedra (`fh guide grid`). It is not a layered model, and its axis is not z. The reference is the beam theory (units SI-mm).

## Model

| Item | Value |
|---|---|
| Bar | x 0 to L = 100 mm, section a circle of radius R = 5 mm, centre (y, z) = (0, 0) |
| Material | E = 210000 MPa, ν = 0.3 |
| Root, x = 0 | ux = uy = uz = 0 |
| Tip, x = L | traction −1 MPa in z, so P = A · 1 MPa |

## Reference

The solver makes the circle a polygon of n = 64 points on the circle, and the reference uses the area and the moment of inertia of that polygon. The polygon is n triangles with the apex at the centre, so A = (n / 2) R² sin(2π/n) = 78.41371 mm². The moment of inertia of the n triangles about a diameter is I = (n R⁴ / 24) sin(2π/n) (2 + cos(2π/n)) = 489.2991 mm⁴, 0.32 % less than π R⁴ / 4 = 490.8739 mm⁴ of the circle. P = 78.41371 N, G = E / (2 (1 + ν)) = 80769.23 MPa.

The tip deflection of the Timoshenko beam is the bending part plus the shear part:

δ = P L³ / (3 E I) + P L / (κ G A) = 0.2543766 + 0.0013968 = 0.2557734 mm,

with the shear coefficient of a circle κ = 6 (1 + ν) / (7 + 6 ν) = 0.8864 (G. R. Cowper, The shear coefficient in Timoshenko's beam theory, J. Appl. Mech. 33 (1966) 335-340). The probe `tip` is at the centre of the tip face, so `tip.uz` = −0.2557734 mm.

The bending stress is σxx = M z / I, with M = P (L − x): the exact Saint-Venant solution away from the root and the tip (S. P. Timoshenko and J. N. Goodier, Theory of Elasticity, 3rd ed., McGraw-Hill 1970, section 120). At x = 50 mm, z = ±5 mm (points of the polygon): σxx = ±78.41371 · 50 · 5 / 489.2991 = ±40.06431 MPa.

## Mesh study

| max_cell (mm) | unknowns | tip.uz (mm) | middle_top.sxx (MPa) | middle_bottom.sxx (MPa) |
|---:|---:|---:|---:|---:|
| 10 | 58,020 | −0.251702 | 39.894 | −40.754 |
| 5 | 116,040 | −0.253386 | 40.023 | −40.229 |
| 2.5 | 232,080 | −0.253973 | 40.053 | −40.104 |
| 1.25 | 479,520 | −0.254157 | 40.061 | −40.075 |

The 64 segments of the circle fix the mesh of the section (450 triangles), so max_cell sets only the planes along x. Each change of tip.uz is 0.31 times the last. The extrapolation is −0.254240 mm, 0.60 % less than the Timoshenko beam: the clamp at the root also stops the warping and the Poisson contraction of the section, which a beam does not. The box bar of `grid-cantilever` shows the same effect, 0.66 %.

## Tolerance

For tip.uz, the clamp gives 0.60 % and the discretization at max_cell 5 mm 0.34 %. Their sum is 0.94 %. The stresses are within 0.42 % of the reference at max_cell 5 mm. The tolerance is 1.5 %. The solve took 13 s in the test build, on a laptop that ran other work.
