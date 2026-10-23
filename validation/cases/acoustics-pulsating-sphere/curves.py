"""The curves of the report of acoustics-pulsating-sphere (scripts/validation-reports.py).

    python3 curves.py OUT 500hz=FOLDER

FOLDER is the output folder of the run of 500hz. The script writes OUT/pressure.csv: the real and
the imaginary part of p at each node of the mesh against the distance r from the centre, with the
analytical p(r) of derivation.md on the same r. getdp/p.pos gives p: for each tetrahedron, the
4 corners, then the real part and the imaginary part at the 4 corners. Only the Python standard
library.
"""

import cmath
import math
import re
import sys
from pathlib import Path

# derivation.md: a = 0.1 m, U = 1e-3 m/s, c = 343 m/s, rho = 1.2 kg/m3, f = 500 Hz.
A, U, C, RHO, F = 0.1, 1e-3, 343.0, 1.2, 500.0
K = 2 * math.pi * F / C


def analytical(r):
    """p(r) = rho c U z (a / r) exp(-j k (r - a)), z = j ka / (1 + j ka)."""
    z = 1j * K * A / (1 + 1j * K * A)
    return RHO * C * U * z * (A / r) * cmath.exp(-1j * K * (r - A))


def nodes(pos):
    """{corner: complex p} of the view in a .pos file of scalar tetrahedra with 2 steps."""
    out = {}
    for m in re.finditer(r"SS\(([^)]*)\)\{([^}]*)\}", pos.read_text()):
        xyz = [float(v) for v in m[1].split(",")]
        p = [float(v) for v in m[2].split(",")]
        for i in range(4):
            out[tuple(round(v, 9) for v in xyz[3 * i : 3 * i + 3])] = complex(p[i], p[4 + i])
    return out


def main(out, folders):
    run = Path(dict(f.split("=", 1) for f in folders)["500hz"])
    rows = sorted(((math.hypot(*xyz), p) for xyz, p in nodes(run / "getdp/p.pos").items()), key=lambda row: row[0])
    worst = max(abs(p - analytical(r)) / abs(analytical(r)) for r, p in rows)
    (Path(out) / "pressure.csv").write_text(
        "# title: Sound pressure against the distance from the centre of the sphere, 500 Hz\n"
        "# y: p [Pa]\n"
        "# note: The model lines go through all the nodes of the mesh, in the order of r. "
        f"The largest difference from the analytical p at a node is {100 * worst:.2f} % of |p| there.\n"
        "# note: Analytical: p(r) = rho c U z (a / r) exp(-j k (r - a)), Kinsler et al. (2000), section 7.1. "
        "At r = a, p is rho c U z.\n"
        "r [m],Re p model,Im p model,Re p analytical,Im p analytical\n"
        + "".join(
            f"{r:.5f},{p.real:.6g},{p.imag:.6g},{analytical(r).real:.6g},{analytical(r).imag:.6g}\n" for r, p in rows
        )
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
