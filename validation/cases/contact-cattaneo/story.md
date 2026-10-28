## Conclusion

The CalculiX model of the cylindrical steel pad (R = 20 mm) on the steel block gives the partial slip of Cattaneo (1938): the centre of the contact sticks, and its 2 edges slip. The shear traction at the centre is q(0) = 433.064 MPa, 0.43 % above the reference 431.203 MPa. The largest contact pressure is 2888.39 MPa, 0.48 % above the Hertz pressure p0 = 2874.69 MPa. The tolerance is 2 %.

The pad gets the normal force P = 4500 N and then the tangential force Q = 0.75 μP = 1012.5 N, each for 1 mm of length. The theory then gives a contact of half-width a = 0.997 mm with a stick zone |x| < c = 0.5 a. The plot shows the contact pressure p and the shear traction q at the nodes of the pad, with the curves of Hertz and Cattaneo:

<!--curve traction.csv-->

At the nodes with |x| ≥ 0.52 a, q = μp: these nodes slip. At the nodes with |x| ≤ 0.48 a, q is less than μp: these nodes stick. A model with no slip (μ = 10) gives q(0) 24.83 % below the reference. Thus the check of q(0) finds a contact that does not slip (see "Tolerance").

The model view shows the plane of the model near the contact, from x = -2 to 2 mm. The pad is above y = 0, and the block is below it. The dots mark the ends of the stick zone, x = ±c, and the edges of the contact, x = ±a. Choose a stress above the model:

- Shear stress S12: Q goes from the pad into the block through the contact. On the top face of the block, S12 is the shear traction q. The check `q_centre` is the mean of S12 in the 2 block elements at x = 0.
- Normal stress S22: on the top face of the block, S22 is -p. Its magnitude is largest at x = 0 and goes to 0 at x = ±a.
