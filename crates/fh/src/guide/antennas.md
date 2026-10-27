# Antennas with openEMS

For an antenna request, for example the input impedance, S11 or the radiation pattern of a dipole, a monopole or a patch, write an openEMS case folder and solve it with `fh run CASE`. openEMS is an FDTD solver: 1 run with a pulse gives all the frequencies of a band.

1. Case: write `openems.py`, a script for the Python modules `CSXCAD` and `openEMS`. Run the solver with `FDTD.Run("sim", cleanup=True)`, in the case folder. `Run` changes the working folder to `sim`: change it back with `os.chdir` before you write the output.
2. Solve: `fh run CASE`. `fh run` starts the Python of openEMS with `openems.py`. The output goes to `log.python`.
3. Numbers: write text tables with a header line `# name name ...`, for example f, R, X and S11 over the band. `fh run` prints the folder of these files.
4. Fields for the page: write `.pos` files, as for GetDP (`fh guide electromagnetics`), for example the far-field pattern as a surface of `SQ` quads with the view `D`. All the views must have the same elements.

This setup agrees with the King-Middleton impedance of a dipole (the validation case `dipole-king-middleton`):

- `SetGaussExcite(f0, fc)` excites the band f0 - fc to f0 + fc. `PML_8` on the 6 faces, a quarter wavelength of the lowest frequency of interest from the antenna.
- Metal as a sheet: a box with 0 thickness. A strip of width w acts as a round wire of radius w/4.
- Mesh lines at a sheet edge: 1 line d/3 in the metal and the next line 2d/3 out of it (d is the cell). Without this rule, the reactance of a strip with 4 cells across its width has an error of 5 %, in place of 0.3 %. `SmoothMeshLines` to a twentieth of the shortest wavelength.
- `AddLumpedPort` across the feed gap. After the run, `port.CalcPort("sim", f)`: Z = `uf_tot / if_tot`, S11 = `uf_ref / uf_inc` for the port resistance. Z does not change with the port resistance.
- Far field: `CreateNF2FFBox(frequency=[f])` before the run, then `CalcNF2FF("sim", f, theta, phi)` on the full sphere: `Dmax` is the directivity and `Prad` the radiated power. For a metal antenna, `Prad` must be equal to the power into the port, 0.5 Re(V I*), within 1 %.
