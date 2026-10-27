# Optics and photonics with Meep

For a light request, for example the scattering of a particle, the transmission of a layer stack or a waveguide, or the field near a small structure, write a Meep case folder and solve it with `fh run CASE`.

1. Case: write `meep_case.py`, a Meep script (`import meep as mp`). Do not name it `meep.py`, because Python then imports the script in place of Meep. Meep has no units: choose a length a, then the frequency is in c / a. `fh-python` has no Meep: do not run the script yourself.
2. Solve: `fh run CASE`. `fh run` starts `python3 meep_case.py` with the Python of Meep, in a copy of the folder. A failure shows the last lines of the output of Python and the path of the log.
3. Numbers: write a text table, for example `spectrum.txt`: a header line such as `# f T R`, then 1 row for each frequency. `fh run` prints the folder of the table.
4. Field for the page: the page draws only 3D cells. For a 2D field, write `field.pos` with 1 view and 1 thin cube for each grid point, `SH(x1,y1,z1,...,x8,y8,z8){...};`, with 8 equal values for each step. For a field F of `add_dft_fields`, write the real part of F exp(-2 pi i k / 8) at the steps k = 0 to 7 and end the view with `TIME{0,0.125,...,0.875};`. Then the page shows the wave move. Use every second or fourth grid point, so the file stays small.

This setup agrees with the analytical scattering efficiency of a dielectric cylinder from x = 1 to 3 within 0.6 % (the validation case `optics-mie-cylinder`, `res40/meep_case.py`):

- A spectrum from 1 run: a `GaussianSource` with a width that covers the band, flux objects with a list of frequencies, and `sim.run(until_after_sources=mp.stop_when_dft_decayed())`.
- 2 runs for a scattered or a reflected flux. Run 1 has no object: it gives the incident flux (`get_fluxes`) and the incident fields on the flux objects (`get_flux_data`). Then call `sim.reset_meep()` and make run 2 with the object: `load_minus_flux_data` gives the flux of the scattered or reflected fields.
- A scattering cross-section: 1 flux object with the 4 sides of a closed box around the object, with `weight=-1` on the sides at -x and -y, so that the flux is outward. Divide it by the incident intensity: the flux through 1 side in run 1, divided by the length of the side.
- A plane wave: `PML` on all sides, and a line source across the full cell, also in the PML, with `is_integrated=True`. A PML of 2 a for wavelengths up to 6.3 a changed the result by less than 0.1 %.
- Grid: the error is second order. 40 points per a (about 40 points in the shortest wavelength in the material) gave 0.6 %, and 20 points gave 2.4 %. Give the result of a second grid in the report. Keep the average of the permittivity at a surface (the default): without it, the error at 40 points per a was 1.2 %.
- Symmetry: when the structure and the source are even about y = 0, `symmetries=[mp.Mirror(mp.Y)]` halves the cell. A 3D cell has hundreds of times more grid points than a 2D cell: start with a coarse grid.
