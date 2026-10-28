## Conclusion

The GetDP model agrees with the TEAM 7 measurements of Bz on the line A1-B1 at all 6 checked points, within the tolerance of 6 % + 1 G. At omega t = 0, the model is within 4.9 % of the measured values. The largest part of the tolerance is 51 %, at x = 36 mm and omega t = 90 degrees.

The table "Result" below gives the 6 values. `bz0_36` is Bz at x = 36 mm at omega t = 0, and `bz90_36` is Bz at the same point at omega t = 90 degrees. x = 36 mm is above the hole.

The eddy currents in the plate cause a large change of Bz. Without the plate, the coil alone gives 88.3 G at x = 126 mm, and 0 at 90 degrees (see "Check of the solve" below). The 90 degree values are small, thus their tolerance is mostly abs_tol = 1 G, the effect of 0.7 degree of phase at x = 126 mm.

gmsh of another version can make another mesh of the same sizes. Thus the values of this run can differ a little from the tables below, which come from the check mesh of derivation.md.

The eddy current density |j| in the plate is in A/m^2. At omega t = 0, the coil current is at its peak. |j| is largest at the corner of the hole at x = y = 126 mm, the corner nearest to the coil: 1.98e6 A/m^2. At omega t = 90 degrees, the coil current is 0, thus the 90 degree values of Bz come only from the eddy currents. |j| is largest at the same corner: 1.83e6 A/m^2. The checked points are on A1-B1, 15 mm above the plate, at x = 36, 126 and 162 mm.
