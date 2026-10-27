# Particles with LAMMPS

For a particle request, for example a dam break, sloshing or granular flow, write a LAMMPS case folder and solve it with `fh run CASE`. LAMMPS does smoothed particle hydrodynamics (SPH) and the discrete element method (DEM).

1. Case: write `case.lmp`, the LAMMPS input, with `units si`.
2. Solve: `fh run CASE`. `fh run` starts `lmp -in case.lmp -log none`. A failure shows the `ERROR` lines of LAMMPS and the path of `log.lmp`.
3. Numbers: `print "# A B" file out.txt`, then `print "$(v_a) $(v_b)" append out.txt`. `fh run` prints the folder of these files.
4. Particles for the page: 1 dump file, `dump ID GROUP custom N particles.dump id x y z v_speed` with `dump_modify ID time yes`. Each particle shows as a cube, and each column other than `id`, `x`, `y` and `z` is a field. Each snapshot must have the same particles: keep them in the box.

This setup agrees with the measured surge front of a dam break (the validation case `dam-break-martin-moyce`):

- 2D: `dimension 2`, `boundary f f p` and `fix enforce2d`. `atom_style sph` on a square grid with spacing dx.
- `pair_style sph/taitwater` with h = 3 dx, alpha = 0.05 and a speed of sound c0 of more than 10 times the largest flow speed. `fix sph` for the water. The walls are 3 layers of particles with `fix sph/stationary`.
- The water starts at rest with the hydrostatic density of the Tait equation.
- Time step 0.025 h / c0. A larger time step gave a wrong front or lost particles, with no warning.
- `fix halt ... error continue` stops a run when a quantity gets to a value.
