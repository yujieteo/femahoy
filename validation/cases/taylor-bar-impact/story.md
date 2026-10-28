## Conclusion

The explicit model of CalculiX gives the final shape of the Taylor bar within the tolerance of 0.80 mm. At 80 µs, the final length of the model is 21.29 mm and the mushroom radius is 6.72 mm. DYNA3D gives 21.66 mm and 7.02 mm. The model values are 0.37 mm and 0.30 mm smaller, 46 % and 37 % of the tolerance.

<!--curve shape.csv-->

The radius increases fast in the first 30 µs and stops at about 50 µs. The length decreases to the end, but from 70 to 80 µs the top moves only 0.13 mm. Thus the bar is almost at rest at 80 µs, the time of the checks.

A finer mesh gives a longer bar and a larger mushroom. The mesh study of 3 meshes extrapolates the model to 21.58 mm and 7.27 mm (section "Mesh study" below), 0.08 mm and 0.25 mm from DYNA3D. The mesh error of the check mesh is 0.29 mm in the length and 0.55 mm in the radius. These errors are larger than the differences between the extrapolated model and DYNA3D.

The 3D view shows the true shape of the quarter bar. Move the time slider above the view: the dots mark the length and the mushroom radius at each time, 1 step each 10 µs. The colour is `PEEQ`, the equivalent plastic strain.
