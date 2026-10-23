"""The curves of the report of naca23012-flap (scripts/validation-reports.py).

    python3 curves.py OUT d-10=FOLDER d0=FOLDER d10=FOLDER

FOLDER is the output folder of the run of each input. The script writes OUT/cp.csv, the
pressure coefficient on the airfoil, and OUT/flap.csv, the lift and the moment against the flap
angle with the values of Table I of Abbott and Greenberg (1939). Only the Python standard
library.
"""

import re
import sys
import tomllib
from pathlib import Path

# The input, the flap angle, the name in the plot and the checks of the input in check.toml.
FLAPS = [
    ("d-10", -10, "flap 10 up", "cl_up10", "cm_up10"),
    ("d0", 0, "flap 0", "cl_0", "cm_0"),
    ("d10", 10, "flap 10 down", "cl_down10", "cm_down10"),
]


def foam_list(path):
    """The items of the first list of an ASCII OpenFOAM file, as strings."""
    text = path.read_text()
    start = text.index("\n(", text.index("FoamFile")) + 2
    return text[start : text.index("\n)", start)].strip().split("\n")


def wall_cp(foam):
    """(x, -Cp) at the centre of each wall face, around the airfoil from the trailing edge. The
    faces of the wall go around the airfoil in a loop. The model has |U| = 1 and the kinematic
    pressure 0 in the far field, thus Cp = 2 p. p on the wall is the value of the cell
    (zeroGradient)."""
    mesh = foam / "constant/polyMesh"
    boundary = (mesh / "boundary").read_text()
    wall = re.search(r"\bwall\s*\{[^}]*?nFaces\s+(\d+);\s*startFace\s+(\d+);", boundary)
    count, start = int(wall[1]), int(wall[2])
    points = [tuple(map(float, p.strip("()").split())) for p in foam_list(mesh / "points")]
    faces = [list(map(int, f[f.index("(") + 1 : -1].split())) for f in foam_list(mesh / "faces")[start : start + count]]
    owner = [int(o) for o in foam_list(mesh / "owner")[start : start + count]]
    last = max((d for d in foam.iterdir() if re.fullmatch(r"\d+", d.name)), key=lambda d: int(d.name))
    p = [float(v) for v in foam_list(last / "p")]
    loop = [(sum(points[i][0] for i in face) / len(face), -2 * p[cell]) for face, cell in zip(faces, owner)]
    edge = max(range(len(loop)), key=lambda i: loop[i][0])
    return loop[edge:] + loop[:edge]


def coefficients(foam):
    """Cl and CmPitch of the last iteration."""
    lines = (foam / "postProcessing/forceCoeffs/0/coefficient.dat").read_text().splitlines()
    names = lines[[i for i, line in enumerate(lines) if line.startswith("#")][-1]][1:].split()
    values = lines[-1].split()
    return float(values[names.index("Cl")]), float(values[names.index("CmPitch")])


def main(out, folders):
    out = Path(out)
    runs = dict(f.split("=", 1) for f in folders)
    rows = []
    for k, (name, *_) in enumerate(FLAPS):
        cells = [""] * len(FLAPS)
        for x, cp in wall_cp(Path(runs[name]) / "foam"):
            cells[k] = f"{cp:.4f}"
            rows.append(f"{x:.5f}," + ",".join(cells))
    (out / "cp.csv").write_text(
        "# title: The pressure on the airfoil at the angle of attack 0 degrees (simpleFoam, mesh level 2)\n"
        "# y: -Cp [1]\n"
        "# note: The plot shows -Cp, thus the suction on the upper surface is at the top. "
        "Each line goes around the airfoil from the trailing edge.\n"
        "x / c [1]," + ",".join(f[2] for f in FLAPS) + "\n" + "\n".join(rows) + "\n"
    )
    # Table I of the source: cl at the angle of attack 0 degrees and cm about the a.c.
    test = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
    rows = []
    for name, angle, _, cl_test, cm_test in FLAPS:
        cl, cm = coefficients(Path(runs[name]) / "foam")
        rows.append(f"{angle},{cl:.4f},{test[cl_test]['value']},{cm:.4f},{test[cm_test]['value']}")
    (out / "flap.csv").write_text(
        "# title: Lift and moment against the flap angle, at the angle of attack 0 degrees\n"
        "# y: cl and cm about the a.c. [1]\n"
        "# points cl test: Abbott and Greenberg (1939), NACA Report 661, Table I\n"
        "# points cm test: Abbott and Greenberg (1939), NACA Report 661, Table I\n"
        "# note: A positive flap angle is down. A positive cm is nose up.\n"
        "flap angle [degrees],cl,cl test,cm,cm test\n" + "\n".join(rows) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
