## Conclusion

ccx builds the column in 10 lifts with element birth and gives the settlement of the lift theory. At mid-height (z = 5 m), ccx gives 0.02315179 mm, and the lift theory gives 0.0231516 mm. The difference is 8.2e-6 of the reference, and the tolerance is 1e-4. At the top of each lift, the largest difference is 2.0e-7 mm.

The 3D view shows the column after each lift, with the settlement U3 in mm. Each step shows only the lifts in the model in that step.

<!--curve settlement.csv-->

Each lift goes on the deformed column strain free. Thus the top of lift 5 or 6 settles most, and the top of the column settles only 0.0074556 mm. A column with all lifts at once settles 0.02943 mm at mid-height, 27 % more, and most at the top. ccx with `ADD=WITH STRAIN` gives this curve (derivation.md). Thus the check fails if ccx does not add the lifts strain free.
