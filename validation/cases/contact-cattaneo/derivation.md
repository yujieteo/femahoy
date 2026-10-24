# contact-cattaneo derivation

This case checks frictional contact with partial slip (stick-slip): a steel cylinder on a steel block, pushed down and then pulled to the side with less than the slip force (units SI-mm).
The centre of the contact sticks, and its 2 edges slip.
The checks are the largest contact pressure p0 and the shear traction q(0) at the centre of the contact.

## Source

- C. Cattaneo, "Sul contatto di due corpi elastici: distribuzione locale degli sforzi", *Rendiconti dell'Accademia Nazionale dei Lincei* 27 (1938), 342-348, 434-436 and 474-478.
- K. L. Johnson, *Contact Mechanics*, Cambridge University Press, 1985: chapter 4 (the Hertz contact of 2 cylinders) and chapter 7 (tangential loading, the partial slip of Cattaneo and Mindlin).

2 long cylinders of the same material touch along a line, in plane strain.
P is the normal force and Q is the tangential force, each for 1 mm of length.
R is the relative radius (here a cylinder on a flat, thus R is the radius of the cylinder), and 1/E* = 2 (1 - nu²)/E.

- Hertz: the contact half-width is a = (4 P R / (π E*))^(1/2). The pressure is p(x) = p0 (1 - x²/a²)^(1/2), with p0 = 2 P / (π a).
- Cattaneo: Q increases from 0 to a value less than μ P. The zone |x| < c sticks, and the zones c < |x| < a slip.

The shear traction q is the sum of 2 elliptical distributions:

- In the slip zones c < |x| < a: q(x) = μ p(x).
- In the stick zone |x| < c: q(x) = μ p(x) - μ p0 (c/a) (1 - x²/c²)^(1/2).

On a half-plane, an elliptical traction b0 (1 - x²/b²)^(1/2) on |x| < b gives a surface strain that is linear in x, in |x| < b.
The 2 parts of q thus give strains of the same size and opposite signs in |x| < c, and the 2 bodies do not slip there.
The total of q is Q = μ P (1 - c²/a²). Thus c = a (1 - Q / (μ P))^(1/2) and q(0) = μ p0 (1 - c/a).

The 2 bodies have the same material. Thus the pressure causes no tangential slip, and the shear traction does not change the pressure.

This case has E = 210000 MPa, nu = 0.3, R = 20 mm, μ = 0.3, P = 4500 N and Q = 0.75 μ P = 1012.5 N.
These are the values of the formulas:

| Value | Reference |
|---|---:|
| E* | 115384.6 MPa |
| a | 0.99656 mm |
| p0 | 2874.69 MPa |
| c = a/2 | 0.49828 mm |
| q(0) | 431.203 MPa |
| q(0) with no slip, Q / (π a) | 323.40 MPa |

## Model

| Item | Value |
|---|---|
| Pad | the cylinder R = 20 mm, cut at x = -12 mm, x = 12 mm and y = 20 mm. Its lowest point is (0, 0) |
| Block | 40 x 20 mm, the top face at y = 0 |
| Steel | E = 210000 MPa, nu = 0.3 for both parts |
| Elements | CPE4, plane strain, 1 mm thick. `ccx` expands each CPE4 to a C3D8 |
| Mesh | `mesh.py`: squares of 0.04 mm (a/25) for -1.4 < x < 1.4 mm and to 0.6 mm deep in each part. Outside, each element is 1.3 times larger than the one before it. 7004 nodes |
| Support | `BOTTOM`, the bottom face of the block: x and y displacements are 0 |
| Top of the pad | equations: the top stays straight and keeps its length. It can turn. The loads are on its ends `LEFT` and `RIGHT` |
| Contact | `*CONTACT PAIR`, surface to surface. Slave `PADS` (the pad), master `BLOCKS` (the block). Linear penalty 1e7 MPa/mm |
| Friction | `*FRICTION`: μ = 0.3, stick slope 1e7 MPa/mm |

The steps:

1. Move the top of the pad down by 0.02 mm. This starts the contact.
2. Release the top in y and apply P = 4500 N at once: 2250 N on each end of the top.
3. Release the top in x and increase Q from 0 to 1012.5 N in increments of 0.05 of the step. P stays.

Q acts on the top of the pad, 20 mm above the contact. In step 3, a couple F = 20 Q / 24 = 843.75 N on the 2 ends of the top cancels the moment of Q about the centre of the contact.
The theory has no moment about the centre of the contact.
A first model had a top that could not turn. The pad then bent, and the contact moved to 1 side.
In that model, q(0) was +0.15 %, -0.29 % and -1.19 % from the reference for parts of 20, 40 and 80 mm.
With the couple, the contact is symmetric, and the size of the parts changes q(0) by 0.22 % or less (see the tolerance).

The checks are at the end of step 3:

- `p0`: the largest contact pressure (`CSTR`, component 1) of all contact points.
- `q_centre`: the mean of S12 at the 8 integration points of each of the 2 block elements at (0, 0) (set `CENTRE`). On the top face of the block, the traction is (S12, S22). Thus S12 = q at the contact.
  At (0, 0), dS12/dx = 0 and dS12/dy = 0. Thus the mean of the integration points is q(0), with an error of the second order in the element size.

| Check | Reference | Computed | Difference |
|---|---:|---:|---:|
| `p0` | 2874.69 MPa | 2888.39 MPa | +0.48 % |
| `q_centre` | 431.203 MPa | 433.064 MPa | +0.43 % |

The plot of the report shows p and q at the nodes of the pad, with the curves of Hertz and Cattaneo.
q = μ p at the nodes with |x| ≥ 0.52 a, and q < μ p at the nodes with |x| ≤ 0.48 a. The theory gives c = 0.5 a.

## Tolerance

The tolerance is 2 % (`rel_tol = 0.02`).
These runs changed 1 item of the model at a time:

| Run | p0 | q(0) |
|---|---:|---:|
| This case | +0.48 % | +0.43 % |
| Mesh 0.08 mm (a/12) | +0.58 % | +0.14 % |
| Mesh 0.02 mm (a/50) | +0.45 % | +0.54 % |
| Mesh 0.01 mm (a/100) | +0.44 % | +0.56 % |
| Stick slope 1e8 MPa/mm | +0.47 % | +0.09 % |
| Penalty and stick slope 1e8 MPa/mm | +0.82 % | +0.52 % |
| Parts of 40 mm (block 80 x 40, pad 32 wide and 40 high, F for that pad) | +0.44 % | +0.26 % |
| Parts of 80 mm (block 160 x 80, pad 36 wide and 80 high, F for that pad) | +0.43 % | +0.21 % |
| Increments of 0.02 in step 3 | +0.48 % | +0.43 % |

The budget of q(0) is 0.13 % for the mesh (a/25 to a/100), 0.34 % for the stick slope, 0.43 % for the penalty and 0.22 % for the size of the parts.
p0 changes by 0.34 % from the penalty and by 0.04 % from the mesh (a/25 to a/100).
Hertz theory has 2 half-planes. The model has parts of finite size, and the penalty lets the surfaces go into each other a little.

The tolerance rejects these wrong models (real runs):

| Run | q(0) |
|---|---:|
| Stick slope 1e6 MPa/mm (the stick zone slips elastically) | +3.43 % |
| μ = 0.35 in place of 0.3 | -5.76 % |
| μ = 10: no slip | -24.83 % |

The runs with μ = 0.35 and μ = 10 are also within 0.4 % of their own theory (404.9 MPa and 323.4 MPa).

## Files

- `mesh.py` writes `mesh.inp`: `python3 mesh.py`. `--size` sets the element size at the contact, for the mesh study.
- `curves.py` writes the plot of the report from `job.frd` of the run. It stops when `check.toml` gives other values of p0 and q(0) than the formulas.
- `fh run` solves the case in about 25 s.
