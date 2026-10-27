# Acoustics with GetDP

For a sound request at 1 frequency, for example a radiating or vibrating surface, write a GetDP case as in `fh guide electromagnetics` for the sound pressure p: `div grad p + k^2 p = 0`, k = omega / c.

- p is `Form0` with `BF_Node` on `NodesOf[All]` and `BF_Node_2E` on `EdgesOf[All]` (second order). Use Gauss rules of 15 points for tetrahedra and 7 for triangles. The mesh stays first order, so the page shows the `.pos` files. In the validation case, h = wavelength / 14 gave errors of 0.2 % or less, and wavelength / 7 gave 1 %.
- Terms: `[ Dof{d p}, {d p} ]` and `[ -k^2 * Dof{p}, {p} ]` in the air. A rigid wall or a symmetry plane needs no term.
- Normal velocity v on a surface (v positive out of the air): `[ Complex[0, omega * rho * v], {p} ]` on the surface, with `Jacobian Sur`.
- Open region: air to a sphere of radius R about the source, with `[ Complex[1 / R, k] * Dof{p}, {p} ]` on it. It is exact for a spherical wave from the centre. For another source, it reflects a part of the wave: solve with a second, larger R and give the change in the report.
- `StoreInVariable` keeps only the real part. For a complex value, print `Re[{p}]` and `Im[{p}]` as 2 quantities. For the mean on a surface, divide an `Integral` of p by an `Integral` of 1.

This setup agrees with the analytical radiation of a pulsating sphere (the validation case `acoustics-pulsating-sphere`).
