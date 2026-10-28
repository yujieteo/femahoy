## Conclusion

The model passes: its shock angle is 0.071 % and its pressure ratio 0.013 % from the exact oblique shock. Its Mach number, temperature and density behind the shock are 0.19 % to 0.20 % from the theory. These 3 checks use 74 % to 80 % of the tolerance of 0.25 %.

The pressure jump of the model is at the shock of the theory. Behind the shock, the pressure is within 0.3 % of the theory. The shock of the theory is at beta = 36.9449 degrees from the tip: y = 0.5678 at x = 0.755 and y = 0.7520 at x = 1.

<!--curve pressure.csv-->

The tip of the wedge causes the 3 larger differences (see "Mesh study"). There the shock and the wall meet in 1 cell, and the scheme makes too much entropy. The hottest cell is at the wall near the tip, with T = 1.412, 6.8 % above the theory.

The flow carries this hot gas along the wall. On the line x = 0.755 near the wall, the temperature is up to 0.44 % above the theory. There the Mach number is up to 0.35 % below the theory, but the pressure is within 0.07 % of it. The averages of the 3 checks include this layer. The error decreases with the cell size. From the 2 finest meshes of the mesh study, the 5 values extrapolate to within 0.011 % of the theory.

<!--curve behind.csv-->
