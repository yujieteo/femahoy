## Conclusion

The model agrees with the exact conjugate solution. The 3 checked temperatures are 0.0002 K to 0.0043 K below the exact values, and the tolerance is 0.006 K. `heater_T` uses 72 % of the tolerance. The mesh error of the check mesh is -0.0040 K (see "Mesh study"), thus the mesh causes most of the difference.

The model also has the correct temperature across the full section at x = 40 mm, not only at the checked points. In the air, the temperature increases by 3.0 K from the centre line to the interface. In the wall, it increases linearly by 0.48 K to the heater. At the scale of the plot, the model line is on the exact line.

<!--curve profile.csv-->

The air enters at 300 K with a uniform velocity. The heater on 20 ≤ x ≤ 80 mm heats the wall, and the wall heats the air. At the outlet, the bulk temperature of the air is 329.9998 K, and the energy balance gives 330 K. At the station x = 40 mm, the checked temperatures are interface_T = 312.9000 K and heater_T = 313.3800 K. At the outlet x = 90 mm, outlet_T = 329.9998 K.
