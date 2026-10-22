# A plane wave in air scatters on a dielectric cylinder: radius a = 1, index n = 2, with the
# electric field along the axis (derivation.md). The units are a and the speed of light, thus the
# size parameter is x = 2 pi a f. Meep computes the scattering efficiency Q = C / (2 a) in 2 runs:
# the incident wave alone, then the cylinder. The second run subtracts the incident fields of the
# first, so the flux out of a closed box around the cylinder is the scattered power C. The script
# writes spectrum.txt (Q at 41 size parameters), cylinder.txt (the checks) and field.pos (the
# page).
import math

import meep as mp
import numpy as np

A, N = 1.0, 2.0  # radius and index of the cylinder
RES = 40  # grid points per a
X = np.linspace(1.0, 3.0, 41)  # size parameters of the spectrum
CHECKS = [1.0, 2.0, 2.5, 3.0]  # size parameters of the check table
SHOW = 2.5  # size parameter of the field on the page
B = 1.5 * A  # half side of the flux box
DAIR, DPML = 1.0, 2.0  # air between the box and the PML, PML thickness
S = 2 * (B + DAIR + DPML)  # side of the square cell

freqs = X / (2 * math.pi * A)
fcen, fwidth = freqs.mean(), 1.5 * (freqs[-1] - freqs[0])
# A line source across the full cell, also in the PML, gives a plane wave along +x.
source = mp.Source(
    mp.GaussianSource(fcen, fwidth=fwidth, is_integrated=True),
    component=mp.Ez,
    center=mp.Vector3(-0.5 * S + DPML),
    size=mp.Vector3(0, S),
)


def simulation(geometry):
    # The fields are even about y = 0, so Meep solves half of the cell.
    return mp.Simulation(
        cell_size=mp.Vector3(S, S),
        resolution=RES,
        boundary_layers=[mp.PML(DPML)],
        sources=[source],
        symmetries=[mp.Mirror(mp.Y)],
        geometry=geometry,
    )


def flux_box(sim):
    """The flux out of the square of half side B: the weight -1 turns the normal outward."""
    sides = [
        mp.FluxRegion(center=mp.Vector3(-B), size=mp.Vector3(0, 2 * B), weight=-1),
        mp.FluxRegion(center=mp.Vector3(B), size=mp.Vector3(0, 2 * B)),
        mp.FluxRegion(center=mp.Vector3(0, -B), size=mp.Vector3(2 * B, 0), weight=-1),
        mp.FluxRegion(center=mp.Vector3(0, B), size=mp.Vector3(2 * B, 0)),
    ]
    return sim.add_flux(freqs, *sides)


# Run 1: the incident wave in air. Its intensity is the flux through 1 side of the box over the
# side length.
sim = simulation([])
box = flux_box(sim)
side = sim.add_flux(freqs, mp.FluxRegion(center=mp.Vector3(-B), size=mp.Vector3(0, 2 * B)))
sim.run(until_after_sources=mp.stop_when_dft_decayed())
incident = sim.get_flux_data(box)
intensity = np.array(mp.get_fluxes(side)) / (2 * B)
sim.reset_meep()

# Run 2: the cylinder. Meep averages the permittivity in the cells that the surface cuts.
sim = simulation([mp.Cylinder(radius=A, height=mp.inf, material=mp.Medium(index=N))])
box = flux_box(sim)
sim.load_minus_flux_data(box, incident)
inside = mp.Vector3(S - 2 * DPML, S - 2 * DPML)  # the cell without the PML
field = sim.add_dft_fields([mp.Ez], [SHOW / (2 * math.pi * A)], center=mp.Vector3(), size=inside)
sim.run(until_after_sources=mp.stop_when_dft_decayed())
q = np.array(mp.get_fluxes(box)) / intensity / (2 * A)

with open("spectrum.txt", "w") as f:
    f.write("# x Q\n")
    for x, value in zip(X, q):
        f.write(f"{x:.4f} {value:.6f}\n")
with open("cylinder.txt", "w") as f:
    f.write("# " + " ".join(f"Q{x:g}" for x in CHECKS) + "\n")
    f.write(" ".join(f"{q[np.abs(X - x).argmin()]:.6f}" for x in CHECKS) + "\n")

# The total field Ez on the page: its real part at 8 times in 1 period, on every fourth grid
# point. The page draws only 3D cells, so each point is the centre of a cube of side 2 h.
ez = sim.get_dft_array(field, mp.Ez, 0)[::4, ::4]
xs, ys, _, _ = sim.get_array_metadata(dft_cell=field)
xs, ys, h = xs[::4], ys[::4], 2 / RES
with open("field.pos", "w") as f:
    f.write('View "Ez" {\n')
    for i, x in enumerate(xs):
        for j, y in enumerate(ys):
            square = [(x - h, y - h), (x + h, y - h), (x + h, y + h), (x - h, y + h)]
            points = ",".join(f"{cx:.4f},{cy:.4f},{z:.4f}" for z in (0, 2 * h) for cx, cy in square)
            steps = [(ez[i, j] * np.exp(-2j * math.pi * k / 8)).real for k in range(8)]
            values = ",".join(f"{v:.4g}" for v in steps for _ in range(8))
            f.write(f"SH({points}){{{values}}};\n")
    f.write("TIME{" + ",".join(f"{k / 8:g}" for k in range(8)) + "};\n};\n")
