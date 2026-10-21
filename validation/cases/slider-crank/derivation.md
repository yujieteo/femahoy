# slider-crank derivation

This case checks the multibody path: a slider-crank with a motor, solved with Project Chrono through PyChrono.
The reference is a closed-form solution. Thus the tolerance comes from the time-step error of the model and from the smallest model error that the check must find.

## Why this case

No other case of the suite has any of these items:

- Multibody dynamics: the solver is Project Chrono, not CalculiX, OpenFOAM or GetDP.
- Rigid bodies with large motion: the crank turns 2.25 times.
- Joints in a closed loop: crank, rod and slider, with a revolute motor, 2 joints at the rod ends and a prismatic joint.
- A reaction of a constraint: the torque of the motor. It depends on all the masses, the inertia of the rod and the gravity.

The same linkage moves a control surface: an actuator (the slide) pushes a horn (the crank) about the hinge of a flap.

## Reference

The crank O-A turns about O at a constant speed w = 2 pi rad/s (60 rpm), with theta = w t. The rod A-B connects the crank pin A to the slider B on the x axis through O.

| Item | Value |
|---|---|
| Crank | length r = 0.1 m, mass m1 = 0.5 kg, a slender bar: the centre at r/2 |
| Rod | length l = 0.3 m, mass m2 = 1.5 kg, a slender bar: inertia m2 l^2 / 12 about its centre |
| Slider | mass m3 = 2.0 kg |
| Gravity | g = 9.81 m/s^2 along -y |
| Start | theta = 0 at t = 0: the dead centre, with B at x = r + l = 0.4 m |

The rod angle phi follows from r sin(theta) + l sin(phi) = 0. Then the slider position is

x_B = r cos(theta) + l cos(phi).

The time derivatives of these 2 equations give phi', phi'', x_B' and x_B''. The joints have no friction. Thus the power of the motor is the rate of change of the energy of the bodies:

torque w = d/dt (T + V),

where T = 1/2 m2 |v_G2|^2 + 1/2 (m2 l^2 / 12) phi'^2 + 1/2 m3 x_B'^2 + (a constant term of the crank) and V = g (m1 y_G1 + m2 y_G2). G1 and G2 are the centres of the crank and the rod.

`reference.py` computes these values. A second calculation agreed with it to 5e-7 N m: a central difference in theta of T + V, with the velocities from central differences of the positions.

The check reads the values at t = 1.125 s, thus theta = 2.25 turns, or 45 degrees:

| Check | Value | Unit |
|---|---:|---|
| `x_slider` | 0.3622582729 | m |
| `torque` | 1.42695517 | N m |

## Model

The Chrono case is the folder `60rpm`: `chrono.py` for PyChrono 10.0.0. The units are m, kg and s.

| Item | Value |
|---|---|
| System | `ChSystemNSC`, the direct solver `ChSolverSparseQR`, the HHT time stepper with step control off, time step 1 ms |
| Bodies | 3 `ChBody`, with the masses and the inertias of the reference. The bodies have the start velocities of a steady 60 rpm |
| Motor | `ChLinkMotorRotationAngle` between the crank and the ground, angle `ChFunctionRamp(0, w)`. `GetMotorTorque()` is the torque on the crank about z |
| Joints | a spherical joint at A, a universal joint at B with its z axis along the rod, a prismatic joint along x at B |
| Output | `slider-crank.txt`: t, x_slider and torque at each time step. `motion.pos`: the motion of a box for each body at 46 frames, for the viewer |

Problems that the model solves:

- A planar loop of revolute joints has 3 constraints too many in 3D. Then the direct solver fails at the first step. A spherical joint and a universal joint remove these 3 constraints and keep the motion.
- With step control, HHT stopped at its smallest time step on this model ("Reached minimum allowable step size"). A fixed time step converges.
- The start velocities agree with 60 rpm, but the start accelerations of the time stepper do not. Thus the torque has a start transient: 0.26 N m after the first step, less than 2e-5 N m after 0.05 s. The check reads the value at 1.125 s.

## Time-step study

These commands, in the repository root, solve the case with another time step in a scratch folder. Set `FH_PYCHRONO` to the Python of PyChrono first (README.md):

```
mkdir /tmp/slider-crank && cp validation/cases/slider-crank/60rpm/chrono.py /tmp/slider-crank && cd /tmp/slider-crank
sed -i.bak 's/^DT, END = 1e-3,/DT, END = 5e-4,/' chrono.py && "$FH_PYCHRONO" chrono.py && tail -1 slider-crank.txt
```

| Time step | x_slider error (m) | torque (N m) | torque error (N m) | Largest torque error after 0.1 s (N m) |
|---|---:|---:|---:|---:|
| 1.5 ms | 1.4e-10 | 1.42697077 | 1.56e-5 | 3.6e-5 |
| 1 ms (the case) | 1.4e-10 | 1.42696209 | 6.9e-6 | 1.6e-5 |
| 0.5 ms | 1.4e-10 | 1.42695689 | 1.7e-6 | 4.0e-6 |
| 0.25 ms | 1.4e-10 | 1.42695561 | 4.4e-7 | 1.0e-6 |

The torque error decreases 4 times when the time step decreases 2 times: HHT is second order. A Richardson extrapolation of the 1 ms and 0.5 ms values gives 1.426955157 N m, 1.3e-8 N m from the reference.

## Tolerance

The pass rule is |computed - reference| <= rel_tol |reference| + abs_tol.

- The time-step error of the torque at 1 ms is 6.9e-6 N m, 5e-6 of the value. The error of the position is 1.4e-10 m, the tolerance of the constraints.
- rel_tol = 1e-4 is 20 times the time-step error. Thus a small change of Chrono or of the platform passes.
- Each of these model errors fails: 1 % in the crank, rod or slider mass changes the torque by 1.2e-3, 5.3e-3 and 3.5e-3 of its value. g = 9.80665 in place of 9.81 changes it by 1.7e-4 (checked: the case fails with a margin of -8.7e-5 N m). No rod inertia changes it by 1.7e-2.
- abs_tol = 0, because no reference value is near 0.
