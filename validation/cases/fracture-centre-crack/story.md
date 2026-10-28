## Conclusion

The CalculiX model gives the stress intensity factor K_I of the centre crack 0.19 % above the formula of Tada (1971). The model gives K_I = 1053.28 MPa mm^0.5, and the formula gives 1051.27 MPa mm^0.5. The tolerance is 0.5 %.

CalculiX has no output of K for a crack in the mesh. Thus the model gets K_I from the opening v of the crack face at the nodes B and C of the element at the tip. B is at r = L/4 = 0.125 mm from the tip, and C is at r = L = 0.5 mm. K_I = c (4 v_B - v_C), with c = 204513.9 MPa mm^-0.5 (see "K_I from the displacements").

The model is 1/4 of the plate. The crack is on y = 0 from x = 0 to 25 mm, with the crack tip at x = 25 mm. The ligament from x = 25 to 50 mm has uy = 0.

- The quarter plate: the crack face opens, and the ligament stays on y = 0.
- The crack tip: the opening at the node B is v = 0.00256369 mm, and at the node C it is v = 0.00510462 mm. Then K_I = 204513.9 × (4 × 0.00256369 - 0.00510462) = 1053.28 MPa mm^0.5.

The opening of each node near the tip gives a value of K_I too. At the node nearest the tip, r = 0.125 mm, it is 0.25 % below the formula. Farther from the tip, the higher terms of the field make it smaller.

<!--curve opening.csv-->

The finer meshes of the mesh study give K_I 0.10 % to 0.16 % above the formula. An error of the method gives a larger difference. For example, E in place of E' gives -9 %.
