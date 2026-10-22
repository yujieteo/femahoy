# hyperelastic-tube derivation

This case checks the inflation of a thick rubber tube past its pressure maximum (units SI-mm).
The rubber is the Ogden model with 3 terms that Ogden fitted to the tests of Treloar. The deformation is large and not uniform: the inner radius grows to 4 times its first value.

## Source

R. W. Ogden, "Large deformation isotropic elasticity - on the correlation of theory and experiment for incompressible rubberlike solids", Proc. R. Soc. Lond. A 326 (1972) 565-584.
Ogden fitted W = Σ μ_p / α_p (λ1^α_p + λ2^α_p + λ3^α_p - 3) to the simple tension, pure shear and equibiaxial tension data of L. R. G. Treloar, Trans. Faraday Soc. 40 (1944) 59-70:

| p | α_p | μ_p (kg/cm²) | μ_p (MPa) |
|---:|---:|---:|---:|
| 1 | 1.3 | 6.3 | 0.617819 |
| 2 | 5.0 | 0.012 | 0.00117680 |
| 3 | -2.0 | -0.1 | -0.00980665 |

The shear modulus at small strain is μ0 = Σ μ_p α_p / 2 = 4.225 kg/cm² = 0.414331 MPa.

D. M. Haughton and R. W. Ogden, "Bifurcation of inflated circular cylinders of elastic material under axial loading - II. Exact theory for thick-walled tubes", J. Mech. Phys. Solids 27 (1979) 489-512, give the pressure of an incompressible thick tube.
Z. Guo, S. Wang and Y. Fu, "Localised bulging of an inflated rubber tube with fixed ends" (arXiv:2203.04110, 2022), give the same integral and the same constants (as μ_p / μ0 = 1.491, 0.003 and -0.024).

This is the simplest boundary-value problem with an exact solution for this rubber, a large deformation that is not uniform, and a pressure maximum.
The inner face has the stretches (λ, 1/λ, 1): the pure shear of the tests of Treloar.

## Reference

The tube is long and its ends do not move (plane strain). A and B are the inner and outer radii before the load, a and b after the load.
The rubber is incompressible, thus r² - a² = R² - A². At the radius r, the hoop stretch is λ = r/R, the radial stretch is 1/λ and the axial stretch is 1.
With w(λ) = W(λ, 1/λ, 1), the stresses are σ_θ - σ_r = λ w'(λ). The equilibrium dσ_r/dr = (σ_θ - σ_r)/r and dr/r = -dλ / (λ (λ² - 1)) give:

p = ∫ from λ_b to λ_a of w'(λ) / (λ² - 1) dλ, λ_b² - 1 = (A/B)² (λ_a² - 1)

For the Ogden energy, w'(λ) = Σ μ_p (λ^(α_p - 1) - λ^(-α_p - 1)). At λ = 1, the integrand has the finite limit μ0.
`reference.py` calculates the integral with Simpson's rule and 4000 intervals. For the Neo-Hooke energy, the same code agrees with the closed form p = μ0 [ln(λ_a/λ_b) + 1/(2 λ_b²) - 1/(2 λ_a²)] to 1e-15.

For A = 10 mm and B = 20 mm, p has a maximum of 0.229790 MPa at λ_a = 2.566. Then p decreases to 0.217814 MPa at λ_a = 4.455, and increases again because of the term with α = 5.

| Check | λ_a | λ_b | p (MPa) | Reference |
|---|---:|---:|---:|---:|
| `f_25` | 2.5 | 1.52069 | 0.229708 | F = p a t = 5.74269 N |
| `f_40` | 4.0 | 2.17945 | 0.219041 | F = p a t = 8.76165 N |
| `u_outer` | 4.0 | 2.17945 | | u(B) = B (λ_b - 1) = 23.5890 mm |

F is the force of the pressure on the inner face of the quarter in the x direction, with a = λ_a A and the thickness t = 1 mm.
`u_outer` checks the volume: it is a function of λ_a only.

## Model

| Item | Value |
|---|---|
| Part | 1/4 of the tube, 0 <= θ <= 90°, plane strain, 1 mm thick |
| Elements | `CPE8R`, 8 through the wall and 16 about the axis, 433 nodes. ccx expands each to a `C3D20R` element. |
| Rubber | `*HYPERELASTIC, OGDEN, N=3`. ccx uses W = Σ 2 μ_i / α_i² (λ̄1^α_i + λ̄2^α_i + λ̄3^α_i - 3) + Σ (J - 1)^(2i) / D_i, thus μ_i = μ_p α_p / 2: 0.40158232, 0.002941995 and 0.00980665 MPa. |
| Compressibility | D1 = 0.0005 MPa⁻¹, thus K = 2 / D1 = 4000 MPa and K / μ0 = 9654. D2 = D3 = 0: ccx then uses (0.1/μ0)² and (0.1/μ0)³ with a warning. Their terms (J - 1)⁴ and (J - 1)⁶ have no effect. |
| `SX`, x = 0 | ux = 0 (symmetry) |
| `SY`, y = 0 | uy = 0 (symmetry) |
| `INNER`, r = A | `*TRANSFORM, TYPE=C` about the z axis. The radial displacement is 15 mm in step 1 (λ_a = 2.5) and 30 mm in step 2 (λ_a = 4.0). The hoop displacement is 0. |
| Steps | 2 `*STATIC` steps of 20 increments. `*HYPERELASTIC` turns on `NLGEOM`. |

A pressure load cannot pass the maximum of p. A `*DSLOAD` of 0.235 MPa on the inner face stopped with "too many cutbacks" at 0.2269 MPa, 1.3 % below the maximum. ccx has no arc length method. Thus the case moves the inner face, and the pressure comes from the reaction forces.

The node sets `F25` and `F40` are the nodes of `INNER` with no node on x = 0. The sum of their x reactions is F.
The node on x = 0 is not in the sets, because its x reaction also holds the hoop force of the symmetry plane.
Its part of F is very small: with D1 = 0.00005 (K / μ0 = 96540), F is within 0.002 % of the reference.
The print requests have `GLOBAL=YES`. Without it, ccx writes the reactions of the nodes of `*TRANSFORM` in their cylindrical system, with an `L` at the end of each row, and `fh validate` does not read these rows.

## Results

| Check | Reference | ccx | Difference |
|---|---:|---:|---:|
| `f_25` | 5.74269 N | 5.74220 N | -0.009 % |
| `f_40` | 8.76165 N | 8.76008 N | -0.018 % |
| `u_outer` | 23.5890 mm | 23.5898 mm | +0.003 % |

The solve takes 3 s.

## Mesh study

| Elements (radial x hoop) | `f_25` | `f_40` | `u_outer` | Time |
|---|---:|---:|---:|---:|
| 4 x 8 | -0.0099 % | -0.0216 % | +0.0038 % | 1 s |
| 4 x 16 | -0.0092 % | -0.0209 % | +0.0034 % | 2 s |
| 8 x 8 | -0.0092 % | -0.0186 % | +0.0036 % | 2 s |
| 8 x 16 (this case) | -0.0085 % | -0.0179 % | +0.0033 % | 3 s |
| 16 x 32 | -0.0084 % | -0.0177 % | +0.0033 % | 13 s |
| 32 x 64 | -0.0084 % | -0.0177 % | +0.0033 % | 64 s |

The mesh of this case is within 0.0002 % of the finest mesh. 10 increments in each step in place of 20 change each result by less than 1e-6.

## Compressibility

The reference is incompressible. The difference of the model comes from K, and it decreases as 1/K:

| D1 (MPa⁻¹) | K / μ0 | `f_25` | `f_40` | `u_outer` |
|---:|---:|---:|---:|---:|
| 0 (ccx uses 0.1 / μ0) | 20 | -3.51 % | -6.03 % | +1.28 % |
| 0.048270 | 100 | -0.79 % | -1.59 % | +0.31 % |
| 0.0048270 | 1000 | -0.081 % | -0.170 % | +0.032 % |
| 0.0005 (this case) | 9654 | -0.0085 % | -0.0179 % | +0.0033 % |
| 0.00005 | 96540 | -0.0009 % | -0.0020 % | +0.0003 % |

## Wrong models

Each wrong model has the mesh and D1 of this case:

| Model | `f_25` | `f_40` |
|---|---:|---:|
| Neo-Hooke with C10 = μ0 / 2 | +14.2 % | +28.8 % |
| Mooney-Rivlin with C10 = 0.18 and C01 = 0.0272 MPa | +14.2 % | +28.8 % |
| Ogden with μ_p of the paper as μ_i of ccx | +43.8 % | +33.9 % |

In plane strain, I1 = I2. Thus a Mooney-Rivlin rubber is a Neo-Hooke rubber with μ = 2 (C10 + C01) in this tube.
Each wrong energy gives the correct `u_outer` (+0.004 %), because `u_outer` checks only the volume.

## Tolerance

The tolerance is 0.2 % (`rel_tol = 0.002`).
The reference is exact for the incompressible rubber. The model has K / μ0 = 9654, which gives 0.018 % or less. The mesh gives 0.0002 % or less, and the increments give no error.
0.2 % is 10 times the total. It passes K / μ0 = 1000 (0.17 %), and it rejects K / μ0 = 100, D1 = 0 and each wrong model.

## Other elements and a sphere

The same tube as a 3D slice, 2 mm thick, with `C3D10` elements from `fh mesh` and uz = 0 on both faces:

- Size 2 mm (5 elements through the wall): `f_25` -0.90 %. Then step 2 stopped with "too many cutbacks". Neo-Hooke on the same mesh solved both steps.
- Size 1.3 mm (8 elements through the wall): `f_25` +0.05 %, `f_40` -0.67 %, `u_outer` +0.02 %, with 169 iterations. `CPE8R` needs 82 iterations.

A thick sphere (a balloon) is the other usual inflation case. This case does not use it.
The Ogden model of ccx calculates the principal stretches from the eigenvalues of C. When 2 stretches are almost equal, the calculation loses digits.
In a sphere, 2 stretches are equal everywhere. An axisymmetric sphere (A = 10 mm, B = 20 mm, 16 x 32 `CAX8R`, K / μ0 = 1000, 0.3 MPa) stopped in the first increment with "too many cutbacks".
The Ogden model with N = 1 and α = 2, which is the Neo-Hooke energy, stopped at 1.3 % of the load. Neo-Hooke solved the step.
With 8 x 16 `CAX8R`, the Ogden model solved the step, and u(A) was 0.003 % from the exact solution. With a first increment of 0.1 in place of 0.05, the 16 x 32 mesh also solved the step.
Thus the result of an Ogden sphere in ccx changes with the mesh and the increments. In the tube, the 3 stretches (λ, 1/λ, 1) are different, and each `CPE8R` mesh solved.

## Files

`mesh.inp` comes from `python3 mesh.py`. The mesh study uses the options of `mesh.py`, for example `python3 mesh.py --radial 16 --hoop 32`.
`python3 reference.py` prints the reference values, the maximum of p and μ0.
