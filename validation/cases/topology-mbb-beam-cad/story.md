## Conclusion

The CAD check of fh gives the compliance 190.48 for the solid of the final design, 1.45 % below the check value 193.27. The check value is the threshold of the same design at the density 0.5, on the elements of the loop. The difference uses 72 % of the tolerance of 2 %. It comes from the smooth boundary of the solid and its finer mesh (section "Mesh of the check" below).

The compliance of the solid is 12.15 % lower than the compliance of the last design of the loop, c = 216.81. A solid with no gray material must be stiffer than the design of the loop. The loop gives an element with the density x the stiffness x^3 E0. In the final design, 588 of the 1200 elements have a density of 0.5 or more, and 259 of these have a density below 0.99. The solid gives all its material the full stiffness E0. It fills 0.510 of the volume.

The model view shows the density of each element of the loop. The CAD makes its solid from the region with a density of 0.5 or more. Move the design slider above the view to see this region at the iterations 1 to 106 of the loop. The slider gives c and the number of elements with a density of 0.5 or more. The marks show the load and the roller. The CAD check puts them on the same corners of the solid. The view does not show the solid: it is `topology/cad/design.step` in the output of the run.
