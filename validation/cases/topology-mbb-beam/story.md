## Conclusion

The topology loop of fh gives the compliance of the published SIMP code for the half MBB beam. After 106 iterations, the loop gives c = 216.8137 with 50 % of the volume. Andreassen et al. (2011) give 216.81. The difference is 0.002 %, less than 1 % of the tolerance of 0.2 %.

The model view shows the density of each element: 1 is material and 0 is void. Move the design slider above the view. It shows the design at the iterations 1, 2, 4, 8, 16, 32 and 64, and the final design at iteration 106. The slider also gives c of each iteration. The marks show the load at the top left corner and the roller at the bottom right corner. The left edge is the symmetry plane.

<!--curve history.csv-->

The compliance falls from 1007.0 for the uniform start design to 228.9 at iteration 20. From iteration 50, it is within 1 % of the final value. The design changes more than c. From iteration 64 to 106, c decreases by only 0.6 %, but 68 of the 1200 elements cross the density 0.5.

<!--curve history-end.csv-->

The stop rule is at its limit in the last iterations. From iteration 95 to 106, the largest change of a density is 0.0098 to 0.0123, and c is 216.67 to 217.06. Iteration 106 is the first with no change above 0.01, thus the loop stops there. A change in the last digit of a solve can stop the loop at another iteration of this band (section "Tolerance" below). Each c of these iterations is in the pass band, at most 0.12 % from the reference.
