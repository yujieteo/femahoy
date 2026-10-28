## Conclusion

The model gives a warpage of -0.1453 mm at the substrate corner at 60 °C. This is 10.3 % smaller than the moiré value of Tsai, Wang and Liu (2021), -0.162 mm. The tolerance is 13 %, the root sum square of the errors of the inputs and of the measurement (see "Tolerance"). Thus the difference is 79 % of the tolerance. The mapped mesh is converged: the 2 mm mesh changes the warpage by 0.009 % (see "Mapped mesh and convergence").

At 60 °C, the strain gauges of the paper give -0.168 mm, and the FEM of the paper gives -0.162 mm. The 2 largest errors of the budget are the zero error of the moiré (9.8 %) and the unknown die size (7.5 %).

The check is at 60 °C only. At 25 °C and 260 °C, the warpage of the model with the former 1 mm C3D10 mesh is 24 % and 22 % smaller than the moiré value (see "Other results" and "Causes of the difference"). Thus this case does not validate the warpage at these temperatures.

u3 is the displacement out of the plane of the quarter package after it cools from 140 °C to 60 °C. The package becomes a dome, and the corner moves to -z. The warpage is u3 at the corner relative to the centre. The lowest point of the substrate is not the corner, but a node on the edge x = 27.5 mm at y = 14.65 mm. It is 0.0047 mm lower than the corner. u3 is 0 at the centre, -0.1453 mm at the corner and -0.1500 mm at the lowest point of the substrate.
