# Shells

Use shells for a thin part.

- Draw the mid-surface in build123d, not the solid. A flat plate is a `Face`. A curved shell is `Face.extrude(edge, direction)` of an arc. Write the STEP as before.
- Mesh: `fh mesh part.step --shell --size H -o mesh.inp ...` writes S6 elements (6-node shell triangles). With `--shell`, `--face NAME=SELECTION` selects edges of the surface, and `--volume NAME=box:...` selects surfaces.
- Section: `*SHELL SECTION, ELSET=ALL, MATERIAL=STEEL`, then the thickness on the next line. A shell node has 6 degrees of freedom: `FIXED, 1, 6` clamps it.
- `*DSLOAD` on an edge surface: `NAME, P, 1.0` is a pressure of 1 MPa on the edge face. The force is P times the thickness times the length of the edge.
- On a symmetry plane, fix the normal translation and 1 rotation about an axis in the plane. For the plane x = c, fix DOF 1 and 5. Do not fix 2 rotations at the nodes of 1 plane: ccx then made the roof 8 times too stiff.
