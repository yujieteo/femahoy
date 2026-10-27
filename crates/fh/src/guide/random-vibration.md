# Random vibration

For a random vibration request, for example a part on a shaker with an acceleration PSD, write 2 steps in `job.inp`. `fh run` then writes the RMS of the response.

1. Modes: `*FREQUENCY, STORAGE=YES`, with the modes up to at least 2 times the highest frequency of the PSD. `STORAGE=YES` keeps the modes for the next step.
2. PSD: an `*AMPLITUDE` with the square root of the PSD, in mm/s2 per sqrt(Hz), at each frequency. For 0.04 g^2/Hz, it is sqrt(0.04) x 9806.65 = 1961.33. `ccx` interpolates the amplitude linearly, thus give a point at least each 10 % of frequency where the PSD slopes.
3. Response: `*STEADY STATE DYNAMICS` with the line `F_LOW, F_HIGH, N, 1.`, then `*MODAL DAMPING` with the line `1, LAST_MODE, ZETA`, then `*BASE MOTION, DOF=3, TYPE=ACCELERATION, AMPLITUDE=PSD` with the support set on the next line. The support set has `*BOUNDARY` in that direction.
4. Points: `ccx` puts N points in each interval between F_LOW, each natural frequency and F_HIGH. Make the spacing at most zeta f1 / 2, with f1 the lowest natural frequency in the band. A spacing of 1.2 zeta f1 gave an RMS 0.5 % too high.
5. Output: `*NODE PRINT` of `U` for each set to know, in the steady state step. A `*NODE FILE` of the modes step stays active, and it writes each frequency to the `.frd` file (70 MB for 2000 nodes). Add `*NODE FILE, FREQUENCY=0` and the line `U` to the steady state step to stop it.
6. Results: `ccx/rms.txt` has, for each node of each print set, the RMS of `U1` to `U3` and of the acceleration `A1` to `A3`. With `*BASE MOTION`, `U` is the absolute displacement: it holds the motion of the base. Thus use `A`. `ccx` prints no stress in this step. Give the peak as 3 times the RMS (3 sigma).
7. Hand check: Miles' equation, A_rms = c sqrt(pi/2 f1 Q W), with Q = 1 / (2 zeta) and W the PSD at f1 in g^2/Hz. c is 1 for a mass on a spring, and 1.566 for the tip of a uniform cantilever. `job.dat` has the effective modal mass of each mode.

This setup agrees within 0.1 % with the modal sum of the beam theory for a cantilever on a shaker (the validation case `random-vibration-cantilever`). Miles' equation gave 1.9 % less there.
