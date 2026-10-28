"""The curves of the report of naca23012-flap (scripts/validation-reports.py).

    python3 curves.py OUT d-10=FOLDER d0=FOLDER d10=FOLDER

FOLDER is the output folder of the run of each input. The script writes Cp, lift, moment, force history, and initial residual CSV files.
The reference values come from Table I of Abbott and Greenberg (1939). Only the Python standard library.
"""

import math
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


def last_time(foam):
    return max((d for d in foam.iterdir() if re.fullmatch(r"\d+", d.name)), key=lambda d: int(d.name))


def wall_surfaces(foam):
    """Split the ordered wall at the leading edge. Geometry identifies each surface."""
    mesh = foam / "constant/polyMesh"
    boundary = (mesh / "boundary").read_text()
    wall = re.search(r"\bwall\s*\{[^}]*?nFaces\s+(\d+);\s*startFace\s+(\d+);", boundary)
    count, start = int(wall[1]), int(wall[2])
    points = [tuple(map(float, p.strip("()").split())) for p in foam_list(mesh / "points")]
    faces = [list(map(int, f[f.index("(") + 1 : -1].split())) for f in foam_list(mesh / "faces")[start : start + count]]
    owner = [int(o) for o in foam_list(mesh / "owner")[start : start + count]]
    p = [float(v) for v in foam_list(last_time(foam) / "p")]
    loop = [(sum(points[i][0] for i in face) / len(face),
             sum(points[i][1] for i in face) / len(face), -2 * p[cell]) for face, cell in zip(faces, owner)]
    return split_wall(loop)


def split_wall(loop):
    """Keep the ordered loop and both surface values at each chord station."""
    edge = max(range(len(loop)), key=lambda i: loop[i][0])
    loop = loop[edge:] + loop[:edge]
    nose = min(range(len(loop)), key=lambda i: loop[i][0])
    branches = [loop[:nose + 1], loop[nose:] + loop[:1]]
    # The wall order supplies the branches. Mean y of the main element supplies their names.
    def height(branch):
        main = [p[1] for p in branch if 0.2 < p[0] < 0.6]
        if not main:
            raise ValueError("wall geometry has no main-element surface")
        return sum(main) / len(main)
    branches.sort(key=height, reverse=True)
    return {name: [[x, cp] for x, y, cp in branch] for name, branch in zip(("upper", "lower"), branches)}


def force_history(foam):
    """Keep full precision and the last record at each repeated iteration."""
    path = foam / "postProcessing/forceCoeffs/0/coefficient.dat"
    if not path.is_file():
        return []
    names, rows = [], {}
    for line in path.read_text().splitlines():
        cells = line.lstrip("# ").split()
        if line.startswith("#"):
            if "Cl" in cells and "CmPitch" in cells:
                names = cells
            continue
        if not names or not cells:
            continue
        try:
            t, cl, cm = float(cells[0]), float(cells[names.index("Cl")]), float(cells[names.index("CmPitch")])
            if all(math.isfinite(v) for v in (t, cl, cm)):
                rows[t] = [t, cl, cm]
        except (ValueError, IndexError):
            continue
    return [rows[t] for t in sorted(rows)]


def residual_history(text):
    """Initial residuals only. A repeated Time block replaces that block, including gaps."""
    rows, current = {}, None
    number = r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?"
    for line in text.splitlines():
        time = re.fullmatch(r"Time = (" + number + ")", line.strip())
        if time:
            current = float(time[1])
            rows[current] = {}
            continue
        if current is None:
            continue
        match = re.search(r"Solving for (p|Ux|Uy|Uz|nuTilda), Initial residual = (" + number + r"),", line)
        if match:
            value = float(match[2])
            if math.isfinite(value) and value > 0:
                rows[current].setdefault(match[1], value)
    return [[t, rows[t]] for t in sorted(rows)]


def csv(out, name, title, y, labels, rows, log=False, gaps=False, points=(), note=""):
    if not rows:
        return
    text = f"# title: {title}\n# y: {y}\n"
    if log:
        text += "# log: y\n"
    if gaps:
        text += "# gaps: break\n"
    for label in points:
        text += f"# points {label}: Abbott and Greenberg (1939), NACA Report 661, Table I\n"
    if note:
        text += f"# note: {note}\n"
    text += ("x / c [1]" if name == "cp" else "flap angle [degrees]" if name in ("lift", "moment") else "iteration [1]")
    text += "," + ",".join(labels) + "\n"
    text += "\n".join(",".join("" if v is None else str(v) for v in row) for row in rows) + "\n"
    (out / (name + ".csv")).write_text(text)


def main(out, folders):
    out = Path(out)
    runs = dict(f.split("=", 1) for f in folders)
    test = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
    cp_rows, load_rows = [], []
    for k, (name, angle, label, cl_test, cm_test) in enumerate(FLAPS):
        foam = Path(runs[name]) / "foam"
        surfaces = wall_surfaces(foam)
        history = force_history(foam)
        residuals = residual_history((foam / "log.simpleFoam").read_text()) if (foam / "log.simpleFoam").is_file() else []
        reference = [test[cl_test]["value"], test[cm_test]["value"]]
        if history:
            load_rows.append([angle, history[-1][1], reference[0], history[-1][2], reference[1]])
        for j, surface in enumerate(("upper", "lower")):
            for x, cp in surfaces[surface]:
                row = [None] * 6
                row[2*k+j] = cp
                cp_rows.append([x, *row])
        for field, index, title in (("cl", 1, "Lift"), ("cm", 2, "Moment")):
            csv(out, f"history-{name}-{field}", f"{title} history, {label}", f"{field} [1]", [field],
                [[row[0], row[index]] for row in history], note="foam/postProcessing/forceCoeffs/0/coefficient.dat")
        equations = [eq for eq in ("p", "Ux", "Uy", "Uz", "nuTilda") if any(eq in row[1] for row in residuals)]
        csv(out, f"residuals-{name}", f"Initial residuals, {label}", "initial residual [1]", equations,
            [[t, *[values.get(eq) for eq in equations]] for t, values in residuals] if equations else [],
            log=True, gaps=True, note="foam/log.simpleFoam (initial residuals)")
    csv(out, "cp", "Pressure on the upper and lower surfaces", "-Cp [1]",
        [f"{f[2]} {surface}" for f in FLAPS for surface in ("upper", "lower")], cp_rows,
        note="Solid: upper surface. Dashed: lower surface. Hinge: x / c = 0.8. Cp = 2p for this case.")
    csv(out, "lift", "Lift against the flap angle", "Cl [1]", ["model Cl", "reference Cl"],
        [[r[0], r[1], r[2]] for r in load_rows], points=["reference Cl"],
        note="Only the 3 marked angles have solved results. Positive flap angle is down.")
    csv(out, "moment", "Moment against the flap angle", "Cm [1]", ["model Cm", "reference Cm"],
        [[r[0], r[3], r[4]] for r in load_rows], points=["reference Cm"],
        note="Moment center: (0.238, 0.07, 0). Positive Cm is nose up.")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
