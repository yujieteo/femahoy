# birth-death-column derivation

This case checks element birth: a concrete column built in 10 lifts, each lift added strain free on the deformed column (units SI-mm).

## Source

The theory is the axial deformation of a bar, δ = N L / (E A), applied to each step of the construction: J. M. Gere and B. J. Goodno, *Mechanics of Materials*, 8th ed., Cengage Learning, 2013, chapter 2 (axially loaded members).
R. W. Clough and R. Woodward, "Analysis of embankment stresses and deformations", *Journal of the Soil Mechanics and Foundations Division*, ASCE 93, 1967, pp. 529-549, analysed embankments built in layers.
J. Kadish, J. R. Barber, P. D. Washabaugh and D. J. Scheeres, "Stresses in accreted planetary bodies", *International Journal of Solids and Structures* 45, 2008, pp. 540-550, use the same principle: new material attaches stress free, and then it is elastic.
They find that the result is very different from the solution for a body that is complete before the load starts.

## Model

| Item | Value |
|---|---|
| Column | 1000 x 1000 mm, height H = 10000 mm, z up |
| Lifts | N = 10 lifts of h = 1000 mm. Lift n is element n (`C3D8`), from z = 1000 (n - 1) to z = 1000 n |
| Concrete | E = 30000 MPa, nu = 0, rho = 2.4e-9 t/mm3 (2400 kg/m3) |
| Weight | `*DLOAD`, `GRAV`, g = 9810 mm/s2 in -z on all elements |
| Base | `BASE`, the 4 nodes at z = 0: all 3 displacements are 0 |
| Steps | 10 steps `*STATIC`, `NLGEOM` |

Step 1 removes lifts 2 to 10 (`*MODEL CHANGE, TYPE=ELEMENT, REMOVE`). Step n adds lift n (`*MODEL CHANGE, TYPE=ELEMENT, ADD=STRAIN FREE`).
The weight of a lift acts only when the lift is in the model.
ccx adds an element strain free only in a nonlinear step: `modelchanges.f` of ccx 2.23 stops with an error in a linear step. Thus the steps have `NLGEOM`.
With nu = 0, the column has a uniaxial stress and no lateral strain. Thus 1 element across the section gives the axial bar of the theory.

The check is the mean of `U`, component 3, of the set `MID` after step 10. `MID` is the 4 nodes at z = 5000 mm, the top of lift 5.

## Theory

A new lift is strain free on the deformed column. Thus the displacement of a point is its displacement after its lift goes on the column.
The column is statically determinate. The axial force at a section is the weight of the lifts above it.
Let W = rho g A h be the weight of 1 lift. Let c = rho g h² / (2E) = W h / (2 E A).
Step j adds lift j. In step j, the top of lift i (z = i h, i <= j) moves down by:

- j > i: all of W goes through 0 <= z <= i h. The top of lift i moves W i h / (E A) = 2 i c.
- j = i: all of W goes through 0 <= z <= (i - 1) h. Lift i also shortens under its own weight by rho g h² / (2E) = c. The top moves (2 i - 1) c.

The sum for j = i to N gives the settlement at the end:

u_i = -c (2 i (N - i) + 2 i - 1) = -c (2 i (N + 1 - i) - 1)

c = 2.4e-9 · 9810 · 1000² / (2 · 30000) = 3.924e-4 mm. At mid-height (i = 5), u_5 = -59 c = -0.0231516 mm.

The largest settlement is at the top of lifts 5 and 6 (z = 5000 and 6000 mm). The top of the column settles only 19 c = 0.0074556 mm.
A column that gets all its lifts at once has u(z) = -(rho g / E) (H z - z² / 2). At the top of lift i, this is -c i (2 N - i). Its largest settlement is at the top: 100 c = 0.03924 mm. At z = 5000 mm it is 75 c = 0.0294300 mm, 27 % more than the column built in lifts.
For thin lifts (h to 0), u_i goes to -rho g z (H - z) / E. This is the settlement of a body that grows by accretion. The lifts add -rho g h (2 z - h) / (2E).
The axial stress is -rho g (H - z) in the 2 columns, because the column is statically determinate. Only the displacement shows the sequence of the construction. Thus the check is a displacement.

A bar of linear elements with the consistent load of its weight gives the exact displacement at its nodes. Each `C3D8` puts W/2 on its bottom face and W/2 on its top face. With nu = 0, the column is this bar.

## Lifts added with strain

The same input with `ADD=WITH STRAIN` in place of `ADD=STRAIN FREE` gives -0.02943027 mm at z = 5000 mm. This is the value of all lifts at once (-0.0294300 mm, +0.0009 %).
A lift added with strain takes the gap between the column and its unloaded top nodes as strain. The end state then does not depend on the sequence.
Thus the check fails if ccx does not add the lifts strain free.

## Mesh study

The second mesh cuts each lift into 2 elements of 500 mm. The model is the same.

| Elements in each lift | u at z = 5000 mm (mm) | Largest change at the top of a lift (mm) |
|---:|---:|---:|
| 1 (this case) | -0.02315179 | |
| 2 | -0.02315179 | 1e-8 |

The change is the last printed digit of ccx (7 digits). Thus the mesh error at the top of each lift is 0, as the theory of the bar elements says.

## Tolerance

The tolerance is 1e-4 of the reference (`rel_tol = 1e-4`), 2.3e-6 mm.
ccx gives -0.02315179 mm, 8.2e-6 of the reference more than the theory. The parts of the difference are:

1. Geometric nonlinearity (`NLGEOM`), about the strain rho g H / E = 7.8e-6. The same input with E 10 times smaller gives 8.3e-5, and with E 10 times larger 8.6e-7. Thus the difference changes with the strain.
2. Mesh: 0 at the top of each lift ("Mesh study").
3. The 7 printed digits of ccx: 4e-7.

The sum is less than 1/10 of the tolerance. The column with all lifts at once, or with lifts added with strain, is 27 % off.

## Plot

`curves.py` writes `settlement.csv`: the settlement at the top of each lift after step 10. The ccx points come from the set `AXIS` in `job.dat` of the run of `job.inp`.
The lift theory goes through each ccx point. The curve of all lifts at once shows the difference that the sequence makes.

## 3D view

`job.inp` writes no field to the `.frd` file. ccx writes the mesh to the `.frd` file only at the first output, with only the elements in the model at that time.
In this case that is lift 1. Thus a 3D view shows lift 1 only, in each step. With `*NODE FILE, OUTPUT ALL`, ccx writes all elements, also the lifts that are not built yet.

## Files

`job.inp` is written by hand: 44 nodes and 10 `C3D8` elements, 1 for each lift. The run takes less than 1 s.
