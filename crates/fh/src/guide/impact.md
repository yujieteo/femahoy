# Impact with explicit dynamics

For an impact or a crash, write `*DYNAMIC, EXPLICIT` in a `*STEP, NLGEOM`. ccx is also the explicit solver.

- Elements: `C3D8R` hexahedra. Write them with a short `fh-python` script, as `validation/cases/taylor-bar-impact/mesh.py` does. `C3D10` from `fh mesh` gave NaN in a plastic impact. `C3D8` locks in plastic flow: it gave a mushroom radius 17 % too small.
- Time increment: the data line is `initial, period, minimum, maximum`. ccx calculates the stable increment 1 time, at the start (`Volumetric COURANT initial stable time increment` in `job.log`). Elements that become flat need a smaller increment. Thus set the initial and the maximum increment to about 1/3 of the stable increment, as `taylor-bar-impact` does (4e-9 s for 1.1e-8 s). A too large increment gives `*ERROR: no convergence in incplas` and NaN.
- Start velocity: `*INITIAL CONDITIONS, TYPE=VELOCITY` with `NALL, 3, V`, then `WALL, 3, 0.0` for the nodes on the wall at t = 0.
- Rigid frictionless wall: `*BOUNDARY` with `WALL, 3`. The face slides on the wall, but it cannot come off the wall.
- Output: ccx has 1 output frequency for `job.frd` and `job.dat`, and the last `FREQUENCY=` of the step sets it. Give the same `FREQUENCY=` to each request. Do not use `TIME POINTS`: in an explicit step it writes each increment.
