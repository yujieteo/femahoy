## Conclusion

The result below compares the largest contact pressure with the Hertz pressure of Johnson (1985). The tolerance is 3 %.

The value is the largest pressure of the 1763 contact points at the end of step 2. Thus it changes with the mesh: a finer mesh gives +0.60 % at the local size 0.25 mm and +0.69 % at 0.13 mm (derivation.md). The Hertz theory has 2 half-spaces, but the ball and the block of the model are finite. Also, the penalty lets the 2 surfaces go into each other a little.

<!--hand-check p0 MPa-->

For a sphere on a flat: E* = E / (2 (1 - nu²)), a = (3 P R / (4 E*))^(1/3), p0 = 3 P / (2π a²).

P = 10000 N, R = 10 mm, E = 210000 MPa and nu = 0.3.  
E* = 210000 / (2 · 0.91) = 115384.6 MPa, a = 0.86624 mm and p0 = 6363.06 MPa.

Scope: Hertz theory uses 2 half-spaces. The model uses finite bodies and penalty contact. The result is the largest contact pressure at step 2.

<!--/hand-check-->
