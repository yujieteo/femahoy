# Measured warpage of a 2.5D package with a metal frame

## Conclusion

The model gives a warpage of -0.1453 mm at the substrate corner at 60 °C. This is 10.3 % smaller than the moiré value of Tsai, Wang and Liu (2021), -0.162 mm. The tolerance is 13 %, the root sum square of the errors of the inputs and of the measurement (see "Tolerance"). Thus the difference is 79 % of the tolerance. The mapped mesh is converged: the 2 mm mesh changes the warpage by 0.009 % (see "Mapped mesh and convergence").

At 60 °C, the strain gauges of the paper give -0.168 mm, and the FEM of the paper gives -0.162 mm. The 2 largest errors of the budget are the zero error of the moiré (9.8 %) and the unknown die size (7.5 %).

The check is at 60 °C only. At 25 °C and 260 °C, the warpage of the model with the former 1 mm C3D10 mesh is 24 % and 22 % smaller than the moiré value (see "Other results" and "Causes of the difference"). Thus this case does not validate the warpage at these temperatures.

u3 is the displacement out of the plane of the quarter package after it cools from 140 °C to 60 °C. The package becomes a dome, and the corner moves to -z. The warpage is u3 at the corner relative to the centre. The lowest point of the substrate is not the corner, but a node on the edge x = 27.5 mm at y = 14.65 mm. It is 0.0047 mm lower than the corner. u3 is 0 at the centre, -0.1453 mm at the corner and -0.1500 mm at the lowest point of the substrate.

## Result

**Pass.** A check passes when |computed - reference| ≤ rel_tol · |reference| + abs_tol, with rel_tol = 0.13 and abs_tol = 0.0 (`check.toml`). `fh validate` gives the computed values.

| Check | Unit | Computed | Reference | Difference | Tolerance | Margin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `warpage` | Not specified | -0.145295 | -0.162 | +0.0167046 (+10.3115 %) | 0.02106 | 0.004355 |

Made by `scripts/validation-reports.py` on 2026-10-28, femahoy 9c54611.

The reference is a measured value, not a theory.
Source: M.-Y. Tsai, Y.-W. Wang and C.-M. Liu, *Thermally-Induced Deformations and Warpages of Flip-Chip and 2.5D IC Packages Measured by Strain Gauges*, Materials 14 (2021) 3723.
The paper is open access under CC BY 4.0: <https://doi.org/10.3390/ma14133723>.
The numbers below come from its Figure 1c, Table 1 and Figures 15, 18 and 20.
A value from a figure is the pixel position of the marker or line in the vector figure of the PDF. The reading error is 2 µm or less.

## The part and the measurement

The package is 55 x 55 x 2.73 mm.
A silicon interposer of 41 x 27.5 mm holds an ASIC and 4 side dies. It is on an organic substrate, with C4 bumps in underfill UF2.
Micro bumps in underfill UF1 connect the dies to the interposer. EMC fills the space at the sides of the dies, and a UF2 fillet goes around the module.
A metal frame on an adhesive is on the edge of the substrate. The frame is 3.2 mm wide on 2 sides and 7.4 mm wide on the other 2 sides.

Shadow moiré with 25.4 µm/fringe measured the substrate surface from 25 to 260 °C.
Back-to-back strain gauges on the diagonal measured the strains on cooling from 140 to 25 °C.
The package is flat at 140 °C, and the paper uses 140 °C as the stress-free temperature.
The warpage is the displacement of the substrate corner relative to the centre.

## Inputs

The inputs are the values of Figure 1c and Table 1 of the paper. The case changes none of them.

| Body | z (mm) | Footprint in the quarter (mm) | E (MPa) | nu | alpha (1/K) |
|---|---|---|---:|---:|---:|
| SUB, substrate | 0 to 1.81 | 27.5 x 27.5 | 20000 | 0.42 | 13.2e-6 |
| UF2, C4 bumps and underfill | 1.81 to 1.91 | 20.5 x 13.75 | 7600 | 0.28 | 29e-6 |
| INT, interposer | 1.91 to 2.00 | 20.5 x 13.75 | 169000 | 0.28 | 2.3e-6 |
| UF1, micro bumps and underfill | 2.00 to 2.04 | 19.5 x 12.2 | 7200 | 0.28 | 28e-6 |
| DIE, silicon | 2.04 to 2.73 | 19.5 x 12.2 | 169000 | 0.28 | 2.3e-6 |
| EMC | 2.00 to 2.73 | 20.5 x 13.75 less the die | 23000 | 0.3 | 10.8e-6 |
| FIL, UF2 fillet | 1.81 to 2.73 | 0.9 wide around the module | 7600 | 0.28 | 29e-6 |
| ADH, adhesive | 1.81 to 1.87 | under the frame | 20 | 0.3 | 170e-6 |
| FRAME | 1.87 to 2.745 | 3.2 wide at x = 27.5, 7.4 wide at y = 27.5 | 117000 | 0.3 | 16.9e-6 |

Table 1 gives 2 values for the EMC, the underfills and the adhesive: one below Tg and one above Tg.
The Tg is 140 °C for the EMC and the underfills, and 50 °C for the adhesive.
The step cools the package from 140 °C to 60 °C. On this path, no material crosses its Tg.
Thus the EMC and the underfills use their values below Tg, and the adhesive uses its values above Tg.
All materials are linear elastic and isotropic, as in the paper.

The quarter model uses x = 0 to 27.5 and y = 0 to 27.5 mm. Symmetry gives u1 = 0 at x = 0 and u2 = 0 at y = 0.
The node `ORIGIN` at (0, 0, 0) has u3 = 0. The node `CORNER` is at (27.5, 27.5, 0).
Thus u3 at `CORNER` is the warpage.

The paper does not give these details. The case uses these choices:

| Detail | Choice | Basis |
|---|---|---|
| Dies | 1 silicon block of 39.0 x 24.4 mm, the ASIC and the side dies with no gap | The FEM mesh of the paper shows 1 die block. Figure 1c gives no die size. In Figure 1c the dies fill 0.947 of the width and 0.888 of the height of the module. |
| EMC | Fills the module outside the die block, from the top of the interposer to the top of the die | Figure 1c |
| UF2 fillet | A triangle in section, 0.92 mm high and 0.9 mm wide on the substrate, with mitred corners | Figure 1c shows the fillet up to the top of the die. |
| Adhesive | The footprint of the frame | Figure 1c |
| nu of the EMC | 0.3 | Table 1 gives 0.69. An isotropic material must have nu below 0.5. Table 1 gives 0.3 for the frame and the adhesive. |
| Material of the fillet | UF2 | Figure 1c gives the fillet the colour of UF2. |

## Reference

The reference is -0.162 mm, the moiré value at 60 °C in Figure 20.
At the same temperature the strain gauges give -0.168 mm, and the FEM of the paper gives -0.162 mm.
The sign follows from the model axes. On cooling, the substrate contracts more than the silicon, so the package becomes a dome and the corner moves to -z.

The check uses 60 °C because there, Table 1 gives all inputs with no choice at a Tg.
Of the measured temperatures with this property, 60 °C has the largest warpage.
At 25 °C the adhesive crosses its Tg. The paper does not tell how its FEM uses the 2 values of the adhesive, and this choice changes the result by 22%.
The results at 25 °C and 260 °C are in "Other results".

## Tolerance

The relative tolerance is 0.13. It is the root sum square of these contributions, rounded up from 12.4%:

| Contribution | Basis | Effect |
|---|---|---:|
| Moiré zero | The moiré gives -15.8 µm at 140 °C, where the package is flat and the other 2 methods give 0. The budget uses ±15.8 µm. | 9.8% |
| Die layout | The paper gives no die size. A die block that fills the module gives 7.5% more warpage. Gaps between the dies give less. | 7.5% |
| Mesh | The former C3D10 mesh gives 1.5% more than the 0.5 mm mesh. This budget stays conservative for the mapped mesh. | 1.5% |
| nu of the EMC | nu from 0.25 to 0.45 changes the result by 0.6% or less. | 0.6% |
| Fillet | No fillet changes the result by -0.3%. | 0.3% |

## Former C3D10 result

The check passed on macOS 27.0.1 arm64 with Python 3.12.13, build123d 0.13.0, gmsh 4.15.2-git and CalculiX 2.23.
The solver time is the run time that `fh run` prints. It changes with the load of the machine.

| Mesh size (mm) | Nodes | C3D10 elements | Minimum scaled Jacobian | Warpage (mm) | Measured (mm) | Difference (%) | Solver time (s) |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 4 | 3421 | 2002 | 0.0006 | -0.149052 | -0.162 | -8.0 | 0.34 |
| 3 (former case) | 5104 | 3034 | 0.0015 | -0.147691 | -0.162 | -8.8 | 0.43 |
| 2 | 9829 | 5997 | 0.0034 | -0.146651 | -0.162 | -9.5 | 0.87 |
| 1 | 36759 | 23463 | 0.0003 | -0.145713 | -0.162 | -10.1 | 5.75 |
| 0.5 | 164228 | 109531 | 0.0045 | -0.145454 | -0.162 | -10.2 | 71.1 |

The former C3D10 warpage is 8.8% below the measured value. This is within the 13% tolerance.
The former STEP script, `scripts/cad/package_2p5d.py`, is in the git history before this change.
The minimum scaled Jacobian is low, because `fh mesh` has one size for the full part. Thus the thin layers (0.04 to 0.1 mm) get flat tetrahedra. All Jacobians are positive.

## Other results

These results use the 1 mm mesh and Table 1 with a step change of E and alpha between Tg and Tg + 1.
The thermal strain uses the alpha of each part of the path from 140 °C (a secant alpha in CalculiX).

| Quantity | Moiré | Gauge | FEM of the paper | femahoy | Difference to moiré (%) | Difference to the FEM of the paper (%) |
|---|---:|---:|---:|---:|---:|---:|
| Warpage at 25 °C (µm), Figure 20 | -225.3 | -261.1 | -232.9 | -170.9 | -24.1 | -26.6 |
| Warpage at 60 °C (µm), Figure 20 | -162.0 | -168.4 | -162.0 | -145.7 | -10.1 | -10.1 |
| Warpage at 260 °C (µm), Figure 15 | 136.5 | | 104 | 106.8 | -21.8 | +2.7 |
| k1 at 25 °C (1/mm), Figure 18 | | -0.000688 | -0.000634 | -0.000592 | | -6.6 |
| k2 at 25 °C (1/mm), Figure 18 | | 0.000642 | 0.0006821 | 0.000942 | | +38.1 |
| k3 at 25 °C (1/mm), Figure 18 | | 0.000242 | 0.000246 | 0.000316 | | +28.3 |

Figure 15 gives -223.5 µm (moiré) and -226 µm (FEM of the paper) at 25 °C, near the values of Figure 20.
The curvature is k = (strain at the bottom - strain at the top) / t, as in Figure 17 of the paper.
The strains are along the diagonal, over a gauge length of 5 mm. The paper does not give the gauge length. Gauge lengths of 3 and 7 mm change k1 by 0.2%, k2 by 3.3% and k3 by 1.6% or less.
k1 is at the middle of the die block (t = 2.73 mm), k2 on the substrate between the module and the frame (t = 1.81 mm), and k3 on the frame (t = 2.745 mm).

At 260 °C the profile on the diagonal has the same shape as the FEM of the paper: the highest point is about 31 mm from the centre, and the frame pulls the corner down.

## Causes of the difference

1. At 25 °C the adhesive is below its Tg, and Table 1 gives E = 100 MPa. The frame then holds the substrate more. This gives a corner that is 7 µm higher than the lowest point, and a warpage 24% below the moiré value.
   With the adhesive at E = 20 MPa at all temperatures, the warpage is -209.5 µm (-7.0% to the moiré), k2 is +10% and k3 is +14% to the FEM of the paper.
   In Figures 18 and 20, the FEM of the paper is a straight line from 25 to 140 °C, and the measurements show no change of slope at 50 °C.
   Thus the FEM of the paper possibly does not use the change of the adhesive at its Tg. A viscoelastic adhesive can also stay soft below its Tg for the time of the test.
2. The paper gives no die size. A die block that fills the module gives 7.5% more warpage at 60 °C and 8.5% more at 25 °C.
3. At 260 °C the value of femahoy is 2.7% above the FEM of the paper, but both models are about 22% below the moiré value.
   The 2 linear elastic models with the data of Table 1 agree, so the cause is in the data or in the model type, not in the solve.
   Above 140 °C the EMC and the underfills are above their Tg, where viscoelastic behaviour is usually large.
4. The moiré gives -15.8 µm at the stress-free temperature. This zero error is about 10% of the warpage at 60 °C.
5. The frame has a large effect: with no frame, the warpage at 25 °C is -344 µm. Thus the warpage is sensitive to each input of the frame and the adhesive.

## Mapped mesh and convergence

`mesh.py` writes a tensor grid through every material boundary. Adjacent blocks share corner nodes and middle nodes.
Each block has C3D20R elements, with 4 elements through the substrate and at least 1 through each thin layer.
Each interval uses `max(1, ceil(length / h - 1e-9))` elements, except the substrate thickness.
This grid matches the audit mesh and the transfinite box rule in gmsh.
The UF2 fillet has 3 steps, each 0.3 mm wide. The midpoint of each third sets its height.
The grid preserves the material sets, symmetry sets, `ORIGIN` and `CORNER`.

Reason: The audit shows a 1.6 % mesh error in the former case. The mapped mesh gives a converged result with 5459 nodes.

These results use CalculiX 2.23 on the Mac. Each row passes `fh validate` with the original check and tolerance.

| h (mm) | Elements | Nodes | Warpage (mm) |
|---:|---:|---:|---:|
| 6, this case | 996 | 5459 | -0.1452954 |
| 4 | 1272 | 6830 | -0.1453125 |
| 2 | 3244 | 16019 | -0.1453084 |

The case differs from the 2 mm result by 0.009 % and from the measured value by -10.31 %.
The 13 % tolerance stays the same. The original 1.5 % mesh budget stays conservative.
The fillet approximation stays within the original 0.3 % fillet budget.

## Reproduce

Run `python3 mesh.py` to write the case mesh at h = 6 mm.
Run `python3 mesh.py H` to write another mesh, with H = 4 or 2 for the table.
Run `fh validate --root ROOT` with a copy of this case under `ROOT/validation/cases`.
The script needs only the Python standard library. The material data and the step stay in `job.inp`.

## Choice of the case

The case must be a 2.5D package with a silicon interposer on an organic substrate, preferably large and with a stiffener.
The paper must have a legal free full text and give the geometry, the materials, the temperatures, the measured warpage and an FEM model.

| Candidate | Result |
|---|---|
| Tsai, Wang and Liu, Materials 14 (2021) 3723, 2.5D package with a metal frame | Selected. CC BY. It gives all inputs, moiré and strain gauge data, and a 3D FEM model. |
| Nam et al., ECTC 2022, 2.5D molded interposer on substrate with a stiffener, DOI 10.1109/ECTC51906.2022.00108 | Not selected. Closed access. Unpaywall, Crossref and a web search found no legal free copy. |
| The extremely large 2.5D molded interposer on substrate package, ECTC 2021 | Not selected. Closed access. |
| Modeling and design of 2.5D package with mitigated warpage, ECTC 2018 | Not selected. An FEM study with no measured warpage. |
| Study of warpage and mechanical stress of 2.5D package interposers, IMAPS 2012 | Not selected. Free, but an FEM study with no measured warpage. |
| Influence of stiffener design on co-packaged optics 2.5D packages | Not selected. No legal free full text with measured warpage was found. |
| Tsai et al., IEEE TDMR 2018, the first moiré study of the same 2.5D package (reference 16 of the paper) | Not selected. Closed access. The selected paper gives its moiré data. |

## Limits

The paper has 3 more models:

1. A 3D quarter model of a flip-chip package. It is not a case, so that the suite stays small. With the same method and a 0.5 mm mesh, femahoy gives -58.3 µm at 25 °C and 28.4 µm at 260 °C. The moiré values of Figure 9 are -61.2 and 31.0 µm, and the FEM values of the paper are -58.1 and 28.8 µm.
2. 2D plane stress and axisymmetric beam models for Figures 6 to 8. The paper gives no geometry or materials for them.
3. A 2D axisymmetric model of the flip-chip package for Figures 12 and 13. `fh mesh` makes no axisymmetric elements.

Viscoelastic EMC, underfill and adhesive, and the cure shrinkage, are follow-up work.
