# optics-mie-cylinder derivation

This case checks the optics path: a plane wave of light scatters on a dielectric cylinder, solved with Meep.
The reference is the analytical series solution. Thus the tolerance comes from the grid error and the size of the model, and from the smallest model error that the check must find.

## Why this case

No other case of the suite has any of these items:

- Optics and photonics: the Maxwell equations in the time domain, solved with the FDTD method of Meep.
- A spectrum from 1 run: a short pulse holds all the frequencies, and a Fourier transform at each grid point of a flux surface gives the flux at 41 frequencies.
- A scattering cross-section: a run of the incident wave alone, then a run with the cylinder that subtracts the incident fields. The flux of the difference out of a closed box is the scattered power.
- Perfectly matched layers (PML) on all sides, a mirror symmetry, and the average of the permittivity in the grid cells that the curved surface cuts.

The scattering efficiency of a cylinder or a sphere (the Mie problem) is the usual first check of an FDTD code. It uses each tool of a scattering, transmission or reflection study, and it has an exact reference.

## Sources

- C. F. Bohren and D. R. Huffman, "Absorption and Scattering of Light by Small Particles", Wiley, 1983, ISBN 0-471-05772-X, chapter 8, the infinite cylinder at normal incidence.
  It gives the series solution of a plane wave on an infinite circular cylinder.
- A. F. Oskooi, D. Roundy, M. Ibanescu, P. Bermel, J. D. Joannopoulos and S. G. Johnson, "MEEP: A flexible free-software package for electromagnetic simulations by the FDTD method", Computer Physics Communications 181 (2010) 687-702.
  It describes the solver, its PML and its average of the permittivity at a surface.

The derivation below gives the same result, so that a reader can check it without the book.
The cylinder has the radius a and the index m, in air. The plane wave goes along +x, with the electric field E_z along the axis of the cylinder. With the time factor exp(-i w t), k = w / c and x = k a (the size parameter), the incident wave is a series of cylinder waves:

```
E_z,inc = exp(i k r cos(phi)) = sum_n i^n J_n(k r) exp(i n phi)
E_z,sca = - sum_n i^n b_n H_n(k r) exp(i n phi)        (r > a, H_n is the Hankel function of the first kind)
E_z,in  =   sum_n i^n c_n J_n(m k r) exp(i n phi)      (r < a)
```

E_z and its radial derivative are continuous at r = a. These 2 conditions give, for each n:

```
b_n = (J_n(m x) J_n'(x) - m J_n'(m x) J_n(x)) / (J_n(m x) H_n'(x) - m J_n'(m x) H_n(x))
```

b_-n = b_n. The scattered power for each length of the cylinder, divided by the incident intensity, is the scattering cross-section C (a length). The scattering efficiency is Q = C / (2 a):

```
Q = (2 / x) (|b_0|^2 + 2 sum_{n>=1} |b_n|^2)
```

`reference.py` computes Q. 3 independent checks of `reference.py` agree:

- The extinction efficiency (2 / x) Re(b_0 + 2 sum b_n), from the forward wave, is equal to Q to 1e-15, as it must be for a material with no loss.
- The fields of the series meet both conditions at r = a to 2e-15.
- At x = 0.01, Q is 0.14 % from the small-cylinder limit pi^2 x^3 (m^2 - 1)^2 / 8.

| Item | Value |
|---|---|
| Cylinder | radius a = 1, index m = 2 (for example silicon nitride in the near infrared), no loss, infinite along z |
| Wave | plane wave along +x in air, E along the axis of the cylinder (the TM case) |
| Units | the lengths in a, the frequency f in c / a. Thus x = 2 pi a f |
| Spectrum | x from 1 to 3 (the wavelength from 6.3 a to 2.1 a), 3 resonances of the cylinder |

| Check | Size parameter x | Q |
|---|---:|---:|
| `q_x1` | 1.0 | 2.862930 |
| `q_x2` | 2.0 | 4.293232 |
| `q_x2_5` | 2.5 | 3.613747 |
| `q_x3` | 3.0 | 2.582972 |

Each check finds a different error. At x = 1, Q depends mostly on m^2 - 1. At x = 2 and x = 2.5, Q is on the steep side of a resonance, thus a shift of the resonance changes Q much. At x = 3, Q is near a sharp resonance (x = 3.015), thus a run that stops too early changes Q.

## Model

The Meep case is the folder `res40`: `meep_case.py` for Meep 1.35.0. The units are a and c.

| Item | Value |
|---|---|
| Cell | a square of side 9 a in 2D: the flux box of half side 1.5 a, then 1 a of air and a PML of 2 a on each side |
| Grid | 40 points per a: 42 points in the shortest wavelength in the cylinder (x = 3). Meep averages the permittivity in the cells that the surface of the cylinder cuts |
| Source | a line of E_z across the full cell on the inner face of the PML at the left, also in the PML (`is_integrated=True`). Its pulse is Gaussian, with the centre at the size parameter 2 and a width that covers the size parameters from 1 to 3 |
| Symmetry | `Mirror(Y)`: E_z is even about y = 0, thus Meep solves half of the cell |
| Run 1 | no cylinder. The flux through 1 side of the box gives the incident intensity. Meep keeps the Fourier transforms of the fields on the box |
| Run 2 | the cylinder. `load_minus_flux_data` subtracts the fields of run 1 on the box, thus the flux out of the box is the scattered power |
| End of each run | `stop_when_dft_decayed()`: the Fourier transforms change by less than 1e-11 |
| Output | `spectrum.txt`: Q at 41 size parameters from 1 to 3. `cylinder.txt`: 1 row with the 4 values of the check. `field.pos`: the real part of the total E_z at the size parameter 2.5 at 8 times of 1 period, on every fourth grid point, for the viewer |

Q in the box does not depend on the size of the box, because the cylinder has no loss and the box holds no source.

## Spectrum

| x | Analytical Q | 20 points per a | 40 points per a (the case) | 80 points per a | Error of the case |
|---:|---:|---:|---:|---:|---:|
| 1.00 | 2.8629 | 2.8652 | 2.8638 | 2.8635 | +0.03 % |
| 1.25 | 4.5285 | 4.5296 | 4.5291 | 4.5290 | +0.01 % |
| 1.50 | 3.9710 | 3.9727 | 3.9717 | 3.9715 | +0.02 % |
| 1.75 | 5.1124 | 5.1244 | 5.1158 | 5.1136 | +0.07 % |
| 2.00 | 4.2932 | 4.2784 | 4.2897 | 4.2926 | -0.08 % |
| 2.25 | 3.4006 | 3.4014 | 3.4010 | 3.4010 | +0.01 % |
| 2.50 | 3.6137 | 3.5641 | 3.6019 | 3.6114 | -0.33 % |
| 2.75 | 1.8457 | 1.8039 | 1.8352 | 1.8432 | -0.57 % |
| 3.00 | 2.5830 | 2.5771 | 2.5833 | 2.5837 | +0.01 % |

The largest error of the case at the 41 size parameters is 0.59 %, at x = 2.70. The errors are largest after the resonance at x = 2.405, where Q falls fast. The waves on the grid are a little slower than c, thus the resonances of the model are at a little smaller x.

## Grid study

These commands, in the repository root, solve the case with another grid in a scratch folder. Set `FH_MEEP` to the Python of Meep first (README.md):

```
mkdir /tmp/mie && cp validation/cases/optics-mie-cylinder/res40/meep_case.py /tmp/mie && cd /tmp/mie
sed -i.bak 's/^RES = 40 /RES = 80 /' meep_case.py && "$FH_MEEP" meep_case.py && cat cylinder.txt
```

| Points per a | Error of q_x1 | Error of q_x2 | Error of q_x2_5 | Error of q_x3 | Largest error of the 41 values | Time |
|---:|---:|---:|---:|---:|---:|---:|
| 10 | +0.30 % | -1.39 % | -5.68 % | -4.11 % | 9.3 % | 4 s |
| 20 | +0.08 % | -0.35 % | -1.37 % | -0.23 % | 2.4 % | 9 s |
| 40, the case | +0.03 % | -0.08 % | -0.33 % | +0.01 % | 0.59 % | 15 s |
| 80 | +0.02 % | -0.01 % | -0.07 % | +0.03 % | 0.14 % | 173 s |

The times are the times of Meep 1.35.0 on 1 core of a Mac with 10 cores, with other work on the Mac. The error falls by a factor of 4 for each step: the order of the method is 2 (from the steps 20, 40 and 80: 2.01, 1.97 and 2.00 at the first 3 checks).
The Richardson extrapolation of the values at 40 and 80 points per a is within 3.4e-4 of the analytical Q at each check. This remainder comes from the size of the model.

## Size of the model

Each line is a change of `meep_case.py` at 40 points per a:

| Change | Error of q_x1 | Error of q_x2 | Error of q_x2_5 | Error of q_x3 |
|---|---:|---:|---:|---:|
| None (the case) | +0.032 % | -0.081 % | -0.327 % | +0.012 % |
| PML 1 a, air 0.5 a | +0.006 % | -0.100 % | -0.376 % | -0.058 % |
| PML 3 a, air 2 a | +0.039 % | -0.099 % | -0.364 % | +0.000 % |
| Flux box of half side 1.2 a | +0.004 % | -0.083 % | -0.335 % | +0.005 % |
| No mirror symmetry | +0.032 % | -0.081 % | -0.327 % | +0.012 % |
| Stop at a change of 1e-8 | +0.031 % | -0.081 % | -0.327 % | +0.010 % |
| Stop at a change of 1e-6 | +0.030 % | -0.081 % | -0.327 % | -0.029 % |

The sizes of the cell, the PML and the box change Q by 7e-4 or less. The symmetry changes nothing. An earlier stop changes Q only near the sharp resonance at x = 3.

## Tolerance

The tolerance is `rel_tol = 0.01` and `abs_tol = 0`.

| Cause | Basis | Effect |
|---|---|---:|
| The reference | exact, 3 independent checks of `reference.py` | 0 |
| Grid error | the largest error of the case at a check, `q_x2_5` | 0.33 % |
| Size of the model | the table above | 0.07 % or less |
| Another version of Meep or another platform | the same method and the same grid. A change of the average at the surface can change Q by a part of the grid error | 0.33 % or less |

`rel_tol` = 1 % covers these causes, with a margin of about 3. It also covers all the 41 values of the spectrum (0.59 % or less). No check value is near 0, thus `abs_tol` is 0.

| Check | Model | Analytical | Difference | Tolerance | Part of the tolerance |
|---|---:|---:|---:|---:|---:|
| `q_x1` | 2.863836 | 2.862930 | 9.06e-4 | 2.86e-2 | 3 % |
| `q_x2` | 4.289743 | 4.293232 | -3.49e-3 | 4.29e-2 | 8 % |
| `q_x2_5` | 3.601924 | 3.613747 | -1.18e-2 | 3.61e-2 | 33 % |
| `q_x3` | 2.583280 | 2.582972 | 3.08e-4 | 2.58e-2 | 1 % |

The tolerance stops these errors. Each line is a change of `meep_case.py`:

| Wrong model | q_x1 | q_x2 | q_x2_5 | q_x3 | Largest error |
|---|---:|---:|---:|---:|---:|
| Index 2.02, not 2 | 2.989774 | 4.125316 | 3.318578 | 2.554301 | 8.2 % |
| No average of the permittivity at the surface (`eps_averaging=False`) | 2.854502 | 4.304857 | 3.643024 | 2.552668 | 1.2 % |
| 20 points per a | 2.865249 | 4.278408 | 3.564103 | 2.577093 | 1.4 % |

A radius of 1 grid cell more (a = 1.025) changes the analytical Q by 5 % to 13 % at the checks.

## Not in the check

- The other polarization (H along the axis of the cylinder): the same steps with the source component `Hz` and the coefficients a_n of the series.
- A sphere in 3D: the same steps, but the cell has hundreds of times more grid points. The cylinder in 2D gives the same check of the method in 15 s.
- Materials with loss or dispersion (metals, for example gold): Meep has Drude and Lorentz materials. Then the flux of the total fields into the box, a second flux object, gives the absorbed power.
- Resonance frequencies and Q factors of a cavity (`Harminv`) and waveguide modes (`EigenModeSource`): other tools of Meep that a later case can check.

On a Mac with 10 cores, `fh run` of the case took 15 s.
