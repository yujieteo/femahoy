## Conclusion

Both CalculiX models of the square [0/90/0] plate agree with the exact 3D solution of Pagano (1970) within 0.17 %. The tolerance is 0.5 %. The thin plate (a/h = 100) uses composite shells. The thick plate (a/h = 4) uses 3D layers with 3 elements in each ply.

The thick plate is the hard test. At a/h = 4, the classical laminate theory gives a deflection 78 % too small, and σ_xx is not symmetric about the mid-plane. The 3D layers follow the exact σ_xx through the thickness, with the jumps at the 2 ply interfaces:

<!--curve sx.csv-->

The plots show the stress at the nodes of the result file. Their largest difference from the exact σ_xx is 0.23 MPa at the top face, 1.8 % of the largest σ_xx. The checks use the stress at the integration points (see "Reference values").

The model view shows the 2 quarter plates. The thin plate is at x = 0 to 50 mm, and the thick plate is at x = 60 to 110 mm. Choose a result above the model:

- σ_xx in the thick plate: the face x = 110 mm is the symmetry plane x = a/2. On this face, |σ_xx| at the nodes is up to 12.6 MPa in the 2 plies at 0 degrees. In the ply at 90 degrees in the core, it is less than 0.1 MPa. The mark is on the line of the σ_xx plot.
- Deflection w of the 2 plates: the marks give w at the centre of each plate, the checks `w_thin` and `w_thick`.

The transverse shear stress τ_xz is continuous through the plies. At the supported edge x = 0, the 3D layers follow the exact τ_xz within 0.056 MPa:

<!--curve txz.csv-->

Use composite shells of this material only for a/h of 50 or more. At a/h = 4, the 8 x 8 composite shells give a deflection 26 % too small (see "Composite shell and the span ratio").
