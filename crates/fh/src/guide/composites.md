# Composite laminates

- Ply: `*ELASTIC, TYPE=ENGINEERING CONSTANTS`, with `E1, E2, E3, nu12, nu13, nu23, G12, G13` on the first line and `G23, 0.0` on the second. Axis 1 is the fibre.
- Ply angle θ: 1 `*ORIENTATION` for each angle, `cos θ, sin θ, 0, -sin θ, cos θ, 0`. The first point is on axis 1.
- Composite shell (`S8R` or `S6` only): `*SHELL SECTION, ELSET=..., COMPOSITE`, then 1 line for each layer: `thickness, , material, orientation`. The first layer is on the negative side of the normal.
- A composite shell is too stiff when the laminate is thick. Its deflection was 1 % low at span/thickness 50, 6 % low at 20 and 16 % low at 10. Its transverse shear stress was wrong by a factor of up to 3.
- Thus, below a span/thickness of 50, or for the shear stress between the plies, use `C3D20R` with 3 elements across each ply and 1 `*SOLID SECTION, ..., ORIENTATION=...` for each ply angle.
- `*EL PRINT` gives the stress of an element with an orientation in the axes of the ply (11 is along the fibre) and adds the name of the orientation at the end of each line. Use these stresses for a ply failure index, for example Tsai-Wu or Hashin, in an `fh-python` script. `GLOBAL=YES` gives x, y, z.

This setup agrees within 0.2 % with the exact 3D solution of Pagano (1970) for [0/90/0] plates under a sine load, with shells at span/thickness 100 and with 3D layers at 4 (the validation case `laminate-pagano`).
