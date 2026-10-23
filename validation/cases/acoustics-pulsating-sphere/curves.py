"""The curves of the report of acoustics-pulsating-sphere (scripts/validation-reports.py).

    python3 curves.py OUT 500hz=FOLDER

FOLDER is the output folder of the run of 500hz. getdp/p.pos gives p: for each tetrahedron, the 4
corners, then the real part and the imaginary part at the 4 corners. The script writes:

- OUT/pressure.csv: the real and the imaginary part of p against the distance r from the centre.
  The analytical p(r) of derivation.md is a line, and the model is the points at the nodes on the
  x axis, so that the line shows between them.
- OUT/error.csv: the difference of p from the analytical p at each node of the mesh, in % of |p|.

Only the Python standard library.
"""

import cmath
import math
import re
import sys
from pathlib import Path

# derivation.md: a = 0.1 m, U = 1e-3 m/s, c = 343 m/s, rho = 1.2 kg/m3, f = 500 Hz, R = 0.5 m.
A, U, C, RHO, F, R = 0.1, 1e-3, 343.0, 1.2, 500.0, 0.5
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
    p = nodes(run / "getdp/p.pos")
    by_r = [(math.hypot(*xyz), v) for xyz, v in p.items()]
    error = sorted((r, 100 * abs(v - analytical(r)) / abs(analytical(r))) for r, v in by_r)
    axis = sorted((xyz[0], v) for xyz, v in p.items() if abs(xyz[1]) < 1e-9 and abs(xyz[2]) < 1e-9)
    line = [A + (R - A) * i / 100 for i in range(101)]
    rows = [(r, None, analytical(r)) for r in line] + [(r, v, None) for r, v in axis]
    cell = lambda v, part: "" if v is None else f"{getattr(v, part):.6g}"
    (Path(out) / "pressure.csv").write_text(
        "# title: Sound pressure against the distance from the centre of the sphere, 500 Hz\n"
        "# y: p [Pa]\n"
        f"# points Re p model: GetDP, the {len(axis)} nodes on the x axis\n"
        f"# points Im p model: GetDP, the {len(axis)} nodes on the x axis\n"
        "# note: Analytical: p(r) = rho c U z (a / r) exp(-j k (r - a)), Kinsler et al. (2000), section 7.1. "
        "At r = a, p is rho c U z.\n"
        "r [m],Re p analytical,Im p analytical,Re p model,Im p model\n"
        + "".join(
            f"{r:.5f},{cell(e, 'real')},{cell(e, 'imag')},{cell(v, 'real')},{cell(v, 'imag')}\n"
            for r, v, e in sorted(rows, key=lambda row: row[0])
        )
    )
    (Path(out) / "error.csv").write_text(
        "# title: Difference of the model p from the analytical p at each node\n"
        "# y: |p model - p analytical| / |p analytical| [%]\n"
        f"# points model: GetDP, all the {len(error)} nodes of the mesh\n"
        f"# note: The largest difference is {max(e for _, e in error):.2f} %. "
        "rel_tol of check.toml is 1 %. It applies to the 4 checks.\n"
        "r [m],model\n" + "".join(f"{r:.5f},{e:.4f}\n" for r, e in error)
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
