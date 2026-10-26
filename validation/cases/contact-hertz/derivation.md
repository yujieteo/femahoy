# contact-hertz derivation

This case checks the largest contact pressure of a steel ball on a flat steel block (units SI-mm).

## Source

K. L. Johnson, *Contact Mechanics*, Cambridge University Press, 1985, section 4.2, the Hertz contact of 2 solids of revolution.
For a sphere of radius R on a flat with the force P: a = (3 P R / (4 E*))^(1/3) and p0 = 3 P / (2π a²), with 1/E* = (1 - nu1²)/E1 + (1 - nu2²)/E2.

## Model

| Item | Value |
|---|---|
| Ball | radius R = 10 mm, the lower half |
| Block | 10 x 10 x 10 mm |
| Steel | E = 210000 MPa, nu = 0.3 for both parts |
| Symmetry | 1/4 model: `SYMX` (x = 0) and `SYMY` (y = 0) |
| Support | `FIXED`, the bottom face of the block: all 3 displacements are 0 |
| Contact | `*CONTACT PAIR`, surface to surface, no friction, linear penalty 1e7 MPa/mm. Slave `BCAP` (the ball), master `BTOP` (the top of the block) |
| Load | 10 kN on the ball: 31.831 MPa on the top face `TOP`. The 1/4 model carries 2500 N |

Step 1 moves the top of the ball down by 0.075 mm, the Hertz approach a²/R. This closes the gap of 0.002 mm and starts the contact.
Step 2 removes this displacement and applies the pressure.
The check is the largest contact pressure (`CSTR`, component 1) at the end of step 2.

The hand check beside the result gives the substituted values and the pressure unit.

## Tolerance

The tolerance is 3 % (`rel_tol = 0.03`).
The value is the largest of the pressures at the contact points, so it changes with the mesh:

| Local size (mm) | p0 (MPa) | Difference |
|---:|---:|---:|
| 0.4 (this case) | 6379.5 | +0.26 % |
| 0.25 | 6402.2 | +0.60 % |
| 0.13 (chat run) | 6406.7 | +0.69 % |

Hertz theory has 2 half-spaces. The block and the ball of the model are finite, and the penalty lets the surfaces go into each other a little.

## Chat run

The request asked for the largest contact pressure and the contact radius in plain words.
The run took 1871 s and cost 5.35 USD. The chat gave p0 = 6407 MPa (+0.69 %) and a = 0.89 mm (+2.7 %).
The chat run had these failures:

1. The first model was a 1/2 model with a local size of 0.08 mm. ccx used 3 GB for more than 5 minutes. The operator stopped it and told the chat: less than 1 minute and less than 1 GB.
2. `*CONTACT PRINT` with `CF` stopped ccx 2 times with "no existing contact pair defined". The chat removed `CF`.
3. A model stopped at the time limit of 60 s. The chat then made a 1/4 model with a local size of 0.13 mm. It solved in 47 s.

The harness had no contact pressure. It now reads `CSTR` from `*CONTACT PRINT`.
This case uses the last input of the chat with a local size of 0.4 mm (2387 nodes) in place of 0.13 mm.

## Files

An earlier `fh mesh` made the mesh in `job.inp` from a gmsh `.geo` script. `fh mesh` now takes only STEP.
The script loaded `part.step` (the block and the 1/4 ball, with a gap of 0.002 mm) and names the groups above.
Its size field is 0.4 mm up to 1 mm from the contact point (0, 0, 10). Then the size grows linearly to 2.5 mm at 5.74 mm.
