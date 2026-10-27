# Cracks and fatigue

For a stress intensity factor K_I, put the crack in the mesh and get K_I from the displacements of the crack face.

1. Model: plane strain (`CPE6`, `CPE8`) or plane stress (`CPS6`, `CPS8`). When the crack is on a symmetry plane, model 1 side: fix the normal displacement on the ligament and keep the crack face free. Write the nodes and the elements with an `fh-python` script.
2. Mesh: a ring of 6-node triangles about the tip, then rings of 8-node quadrilaterals. In each triangle, put the mid-side nodes of the 2 edges from the tip at 1/4 of the edge from the tip. Make the edge length L from the tip about 1/50 of the crack length.
3. K_I: on the crack face, B is the node at L/4 from the tip and C is the node at L. With the symmetry plane, K_I = c (4 v_B - v_C), c = E'/4 sqrt(2 pi / L). v is the displacement normal to the crack, E' = E / (1 - nu^2) in plane strain and E' = E in plane stress. Without the symmetry plane, use the opening of the 2 faces for v and E'/8 for E'/4.
4. Add a node K with no element. For a crack along x, write `*EQUATION`, the line `3`, and the line `K, 2, 1.0, B, 2, -4c, C, 2, c` with the numbers. Then `*NODE PRINT` of K writes K_I as U2 in `job.dat`, in MPa mm^0.5. 1 MPa m^0.5 is 31.62 MPa mm^0.5.
5. Fields for the page: write `*NODE FILE, OUTPUT=3D` in the first step. Without `OUTPUT=3D`, ccx writes 2D elements, and the page does not show them.
6. Fatigue life: solve 4 or more crack lengths. Integrate the Paris law da/dN = C dK^m from the first length to the last length in an `fh-python` script. Give C with its units.
7. `*CRACK PROPAGATION` does not model the crack. It takes K from the stress of the part without the crack, with fixed shape factors. Do not use it for K of a crack in the mesh.

This setup agrees within 0.2 % with the handbook value of K_I of a centre crack in a plate (the validation case `fracture-centre-crack`).
