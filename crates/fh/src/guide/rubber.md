# Rubber

Use `*HYPERELASTIC` for rubber. It turns on `NLGEOM`.

- Constants: `NEO HOOKE` takes C10 and D1. `MOONEY-RIVLIN` takes C10, C01 and D1. `OGDEN, N=3` takes mu1, alpha1, mu2, alpha2, mu3, alpha3, D1, D2 on line 1 and D3 on line 2.
- An Ogden fit in a paper often has the energy sum mu_p / alpha_p (l1^alpha_p + l2^alpha_p + l3^alpha_p - 3). ccx has 2 mu_i / alpha_i^2 in place of mu_p / alpha_p. Thus write mu_i = mu_p alpha_p / 2. The shear modulus at small strain is mu0 = sum mu_i, or 2 (C10 + C01).
- Write D1 = 2 / K with K of about 10^4 mu0. Do not write D1 = 0: ccx then uses K = 20 mu0, with only a warning. In the tube below, K = 20 mu0 gave an error of 6 % and K = 100 mu0 an error of 1.6 %. D2 = D3 = 0 has no effect.
- Use `CPE8R`, `CAX8R` or `C3D20R`. A `C3D10` mesh with 5 elements through the wall stopped. With 8 elements, the error was 0.7 %.
- A pressure load stops at a maximum of the pressure, for example in an inflated tube or a balloon. ccx has no arc length method. Thus move the face: `*TRANSFORM, NSET=INNER, TYPE=C` with the axis, then `INNER, 1, 1, UR` and `INNER, 2, 2, 0.0` in `*BOUNDARY`.
- Then get the force from `*NODE PRINT, NSET=..., GLOBAL=YES` with `RF`. Without `GLOBAL=YES`, ccx writes the forces of these nodes in the cylindrical system. Do not put the nodes of a symmetry plane in the set, because their force also holds the symmetry force.
- Ogden with 2 equal stretches, as in a sphere or in equibiaxial tension, can stop with "too many cutbacks". Neo-Hooke on the same mesh converged. Then change the first increment or the mesh, and compare the results.

This setup agrees within 0.02 % with the exact inflation of a rubber tube past its pressure maximum (the validation case `hyperelastic-tube`).
