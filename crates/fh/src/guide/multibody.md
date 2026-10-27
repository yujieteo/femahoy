# Multibody dynamics with Project Chrono

For a mechanism request, for example a linkage, a pendulum or a flap that an actuator turns about its hinge, write a Chrono case folder and solve it with `fh run CASE`.

1. Case: write `chrono.py`, a PyChrono script (`import pychrono as chrono`). Use SI units: m, kg and s. `fh-python` has no PyChrono: do not run the script yourself.
2. Solve: `fh run CASE`. `fh run` starts `python3 chrono.py` with the Python of PyChrono, in a copy of the folder. A failure shows the last lines of the output of Python and the path of the log.
3. Numbers: write a text table, for example `results.txt`: a header line such as `# t x_slider torque`, then 1 row for each time step. `fh run` prints the folder of the table.
4. Motion for the page: write `motion.pos` with 1 view `U`. For each body, write 1 hexahedron, `VH(x1,y1,z1,...,x8,y8,z8){...};`: the 8 corners of a box at the start, then the move of each corner (x, y, z) at each frame. End the view with `TIME{t1,t2,...};`. The page shows the bodies move at true scale.

This setup agrees with the closed-form motor torque of a slider-crank at 60 rpm (the validation case `slider-crank`, `60rpm/chrono.py`):

- `ChSystemNSC` with the direct solver `ChSolverSparseQR` and the HHT time stepper. Set `SetStepControl(False)` on the HHT time stepper: with step control, it stops at its smallest step. A time step of 1 ms gives a torque error of 5e-6.
- A closed loop of joints in a plane: do not use only revolute joints, because they give 3 constraints too many in 3D and the solver fails. Use a spherical joint at one end of the rod and `ChLinkUniversal` at the other end, with the z axis of the universal joint along the rod.
- The z axis of the frame of `ChLinkLockPrismatic` is the slide direction.
- A crank at a constant speed: `ChLinkMotorRotationAngle` with `ChFunctionRamp(0, omega)`. Give each body the start velocity of that motion. The motor torque has a start transient of about 0.05 s. `GetMotorTorque()` is the torque on the first body of the motor, about the z axis of its frame.
