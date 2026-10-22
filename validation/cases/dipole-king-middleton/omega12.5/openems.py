"""A centre-fed strip dipole in free space, solved with openEMS (FDTD).

The strip has the width W and no thickness. It acts as a round dipole of radius A = W / 4, with
the thickness parameter Omega = 2 ln(2 H / A) = 12.5 of the reference (derivation.md). A lumped
port in the gap at the centre excites a Gaussian pulse of 0.2 to 2.8 GHz. Thus 1 run gives the
input impedance over the full band.

Output:
- impedance.txt: f, beta h, R, X and |S11| for 50 ohm, from 0.4 to 2.8 GHz. Below 0.4 GHz the
  pulse has too little energy.
- dipole.txt: 1 row with the values of the check.
- pattern.pos: the far-field pattern at beta h = pi / 2, a surface at the distance D(theta, phi)
  from the centre, with the view D.

The lengths are in mm, as the mesh of openEMS.
"""

import os

import numpy as np
from CSXCAD import ContinuousStructure
from openEMS import openEMS
from openEMS.physical_constants import C0

OMEGA = 12.5
W = 1.2  # strip width
A = W / 4  # equivalent radius
H = A * np.exp(OMEGA / 2) / 2  # half length, 77.70 mm
GAP = 0.6  # feed gap
N = 4  # cells across the strip
F0, FC = 1.5e9, 1.3e9  # the band F0 - FC to F0 + FC
MARGIN = 100.0  # air between the dipole and the PML: a quarter wavelength at beta h = 1.2
SIM = "sim"

d = W / N
fdtd = openEMS(EndCriteria=1e-6)  # -60 dB: -50 dB gives a ripple of R below 0.6 GHz
fdtd.SetGaussExcite(F0, FC)
fdtd.SetBoundaryCond(["PML_8"] * 6)
csx = ContinuousStructure()
fdtd.SetCSX(csx)
mesh = csx.GetGrid()
mesh.SetDeltaUnit(1e-3)

# The edges of a sheet: a mesh line d/3 in the metal and the next line 2d/3 out of the metal.
edge = W / 2 - d / 3
mesh.AddLine("x", list(np.linspace(-edge, edge, N)) + [-W / 2 - 2 * d / 3, W / 2 + 2 * d / 3])
mesh.AddLine("x", [-W / 2 - MARGIN, W / 2 + MARGIN])
mesh.AddLine("y", [-MARGIN, -d, 0, d, MARGIN])
z = list(np.arange(-GAP / 2, GAP / 2 + d / 2, min(d, GAP / 2)))
z += [GAP / 2 + k * d for k in range(4)] + [H - k * d for k in range(4, 0, -1)]
z += [H - d / 3, H + 2 * d / 3, H + MARGIN]
mesh.AddLine("z", z + [-v for v in z])
strip = csx.AddMetal("strip")
strip.AddBox([-W / 2, 0, GAP / 2], [W / 2, 0, H], priority=10)
strip.AddBox([-W / 2, 0, -H], [W / 2, 0, -GAP / 2], priority=10)
port = fdtd.AddLumpedPort(1, 50.0, [-W / 2, 0, -GAP / 2], [W / 2, 0, GAP / 2], "z", 1.0)
mesh.SmoothMeshLines("all", C0 / (F0 + FC) / 1e-3 / 20, 1.3)

# beta h = 2 pi f H / c.
def freq(bh):
    return np.asarray(bh) * C0 / (2 * np.pi * H * 1e-3)

f_half = freq(np.pi / 2)
nf2ff = fdtd.CreateNF2FFBox(frequency=[f_half])
case = os.getcwd()
fdtd.Run(SIM, cleanup=True)
os.chdir(case)  # Run leaves the working folder in SIM.

f = np.linspace(0.4e9, 2.8e9, 241)
port.CalcPort(SIM, f)
z_in = port.uf_tot / port.if_tot
s11 = np.abs((z_in - 50) / (z_in + 50))
bh = 2 * np.pi * f * H * 1e-3 / C0
np.savetxt(
    "impedance.txt",
    np.c_[f / 1e9, bh, z_in.real, z_in.imag, 20 * np.log10(s11)],
    fmt="%.6g",
    header="f_GHz bh R X S11_dB",
)

# The first resonance: X = 0 on a fine band.
fine = np.linspace(freq(1.3), freq(1.7), 2001)
port.CalcPort(SIM, fine)
x = (port.uf_tot / port.if_tot).imag
k = np.flatnonzero((x[:-1] < 0) & (x[1:] >= 0))[0]
f_res = fine[k] - x[k] * (fine[k + 1] - fine[k]) / (x[k + 1] - x[k])
port.CalcPort(SIM, [freq(1.2), freq(1.5), freq(1.7), f_res, f_half])
z = port.uf_tot / port.if_tot
p_acc = 0.5 * np.real(port.uf_tot[4] * np.conj(port.if_tot[4]))

theta, phi = np.arange(0, 181, 2.0), np.arange(0, 360, 5.0)
far = nf2ff.CalcNF2FF(SIM, f_half, theta, phi)
columns = {
    "R12": z[0].real,
    "X12": z[0].imag,
    "R15": z[1].real,
    "X17": z[2].imag,
    "bh_res": 2 * np.pi * f_res * H * 1e-3 / C0,
    "R_res": z[3].real,
    "D": far.Dmax[0],
    "eff": far.Prad[0] / p_acc,
}
with open("dipole.txt", "w") as out:
    out.write("# " + " ".join(columns) + "\n")
    out.write(" ".join(f"{v:.6g}" for v in columns.values()) + "\n")

# The pattern for the page: D on the grid of theta and phi, on 1 quad for each cell.
D = far.Dmax[0] * far.E_norm[0] ** 2 / np.max(far.E_norm[0]) ** 2
t, p = np.meshgrid(np.radians(theta), np.radians(np.append(phi, 360.0)), indexing="ij")
D = np.c_[D, D[:, :1]]
xyz = np.stack([D * np.sin(t) * np.cos(p), D * np.sin(t) * np.sin(p), D * np.cos(t)], axis=-1)
with open("pattern.pos", "w") as out:
    out.write('View "D" {\n')
    for i in range(len(theta) - 1):
        for j in range(len(phi)):
            corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
            # A cell at a pole is a triangle: its corners at the pole are 1 point.
            if i == 0:
                corners.pop(3)
            elif i == len(theta) - 2:
                corners.pop(2)
            points = ",".join(f"{c:.5g}" for ij in corners for c in xyz[ij])
            values = ",".join(f"{D[ij]:.5g}" for ij in corners)
            out.write(f"S{'Q' if len(corners) == 4 else 'T'}({points}){{{values}}};\n")
    out.write("};\n")
