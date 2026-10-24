"""The curve of the report of submodel-kirsch-hole (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The run solves global.inp and job.inp in
FOLDER/ccx. The script writes OUT/line-x0.csv: sigma_xx on the line x = 0 from the hole, at the
nodes of global.frd and job.frd, with the Kirsch solution. Only the Python standard library.
"""

import sys
from pathlib import Path

# derivation.md: the tension S along x and the radius A of the hole, in MPa and mm. The cut of
# the submodel is at r = CUT, and the global model goes to y = EDGE.
S, A, CUT, EDGE = 100.0, 10.0, 20.0, 50.0


def line_x0(path):
    """{y: S11} at the nodes of x = 0 in the result file path. ccx writes each node of a 2D
    element 3 times through the thickness (OUTPUT=3D): the value is the mean of the 3."""
    nodes, s11, block = {}, {}, None
    for line in open(path):
        if line.startswith("    2C"):
            block = "nodes"
        elif line.startswith(" -4  STRESS"):
            block = "stress"
        elif line.startswith((" -3", " -4")):
            block = None
        elif line.startswith(" -1") and block == "nodes":
            nodes[int(line[3:13])] = (float(line[13:25]), float(line[25:37]))
        elif line.startswith(" -1") and block == "stress":
            s11[int(line[3:13])] = float(line[13:25])
    values = {}
    for n, v in s11.items():
        x, y = nodes[n]
        if abs(x) < 1e-6:
            values.setdefault(round(y, 6), []).append(v)
    return {y: sum(v) / len(v) for y, v in values.items()}


def kirsch(y):
    """sigma_xx = sigma_thetatheta on x = 0, at r = y."""
    q = A * A / (y * y)
    return S * (1 + q / 2 + 3 * q * q / 2)


def error(v, y):
    return f"{(v / kirsch(y) - 1) * 100:+.2f} %"


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    ccx = Path(runs["job.inp"]) / "ccx"
    glob, sub = line_x0(ccx / "global.frd"), line_x0(ccx / "job.frd")
    ys = sorted(set(glob) | set(sub) | {A + (EDGE - A) * k / 80 for k in range(81)})
    cell = lambda c, y: f"{c[y]:.4f}" if y in c else ""
    rows = "".join(f"{y:g},{kirsch(y):.4f},{cell(glob, y)},{cell(sub, y)}\n" for y in ys)
    (Path(out) / "line-x0.csv").write_text(
        "# title: Plate with a hole, σ_xx on the line x = 0 from the hole\n"
        "# y: σ_xx [MPa]\n"
        "# points global model: the nodes of x = 0 in global.frd, 16 elements\n"
        "# points submodel: the nodes of x = 0 in job.frd, the ring from the hole to the cut at y = 20 mm\n"
        "# note: Kirsch is the exact solution of the infinite plate. The plot shows the nodal S11 of the 2 models.\n"
        f"# note: At the hole, y = 10 mm, the global model gives {glob[A]:.1f} MPa ({error(glob[A], A)}) "
        f"and the submodel {sub[A]:.1f} MPa ({error(sub[A], A)}). At the cut, y = 20 mm, the submodel "
        f"gives {error(sub[CUT], CUT)}: there it has the error of the global model.\n"
        "y [mm],Kirsch,global model,submodel\n" + rows
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
