## Conclusion

The coupled model of OpenFOAM, CalculiX and preCICE passes FSI1 of Turek and Hron (2006). The drag and the lift are 0.33 % and 0.18 % below the reference. The displacement of point A is 3.1 % (ux) and 1.9 % (uy) below the reference. `ux_a` uses 77 % of the tolerance of 4 %.

The coarse mesh of the check causes most of the difference in the displacement. On the finer meshes of the mesh study, the drag and the displacements move to the reference. The lift stays within 0.18 % of it. At zero cell size, the drag and ux are within 0.13 % of the reference, and uy is 0.70 % below it. The coupling is necessary: a rigid flag gives a lift 46 % above the reference (see "Tolerance").

The flow around the cylinder bends the flag up to a steady shape. Point A, the middle of the free end of the flag, moves 0.81 mm up. Point A before the deformation is at x = 0.6 m, y = 0.2 m. The pressure `p` gives 92 % of the lift.

The coupling converges to a steady state. The inlet velocity increases in the first 2 time windows. From window 28 to window 30, uy changes by 0.011 %, and the other values by 0.0011 % or less. The section "Mesh study" estimates the rest of the change after window 30 as +0.05 % in uy.

<!--curve history.csv-->
