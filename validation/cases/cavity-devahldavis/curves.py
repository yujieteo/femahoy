"""The curves of the report of cavity-devahldavis (scripts/validation-reports.py).

    python3 curves.py OUT ra1e4=FOLDER ra1e5=FOLDER

FOLDER is the output folder of the run of each input. The script writes OUT/u.csv, the
horizontal velocity on the vertical centre line, and OUT/v.csv, the vertical velocity on the
horizontal centre line, with the maxima of Table V of de Vahl Davis (1983). Only the Python
standard library.
"""

import re
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import vtu  # noqa: E402

RUNS = [("ra1e4", "Ra 1e4"), ("ra1e5", "Ra 1e5")]
CHECKS = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
# The position of each maximum of Table V of the source: only the table of derivation.md, "Sources",
# has it. The maxima come from check.toml.
U_MAX = [(0.823, CHECKS["umax_ra1e4"]["value"]), (0.855, CHECKS["umax_ra1e5"]["value"])]
V_MAX = [(0.119, CHECKS["vmax_ra1e4"]["value"]), (0.066, CHECKS["vmax_ra1e5"]["value"])]


def last_step(folder):
    steps = sorted(folder.glob("step-*.vtu"), key=lambda p: int(re.search(r"\d+", p.stem)[0]))
    return vtu.read(steps[-1])


def centre_lines(folder):
    """(y, u) of the cells on x = 0.5 and (x, v) of the cells on y = 0.5. The mesh has an odd
    number of cells, thus each centre line goes through a row of cell centres."""
    step = last_step(folder)
    u, v = [], []
    for k, velocity in enumerate(step.cell["U"]):
        nodes = step.nodes(k)
        x = sum(step.points[i][0] for i in nodes) / len(nodes)
        y = sum(step.points[i][1] for i in nodes) / len(nodes)
        if abs(x - 0.5) < 1e-4:
            u.append((y, velocity[0]))
        if abs(y - 0.5) < 1e-4:
            v.append((x, velocity[1]))
    return sorted(u), sorted(v)


def write(path, title, y, x, lines, table):
    names = [label for _, label in RUNS]
    rows = []
    for k, line in enumerate(lines):
        for a, b in line:
            cells = [""] * (len(names) + 1)
            cells[k] = f"{b:.5g}"
            rows.append(f"{a:.5f}," + ",".join(cells))
    for a, b in table:
        rows.append(f"{a}," + "," * len(names) + f"{b}")
    path.write_text(
        f"# title: {title}\n"
        f"# y: {y}\n"
        "# points Table V maximum: de Vahl Davis (1983), Table V\n"
        "# note: The model has no units: L, Th - Tc and alpha are 1. The velocity is in units of alpha / L.\n"
        f"{x}," + ",".join(names) + ",Table V maximum\n" + "\n".join(rows) + "\n"
    )


def main(out, folders):
    out = Path(out)
    runs = dict(f.split("=", 1) for f in folders)
    lines = [centre_lines(Path(runs[name])) for name, _ in RUNS]
    write(out / "u.csv", "The horizontal velocity on the vertical centre line x = 0.5",
          "u L / alpha [1]", "y / L [1]", [u for u, _ in lines], U_MAX)
    write(out / "v.csv", "The vertical velocity on the horizontal centre line y = 0.5",
          "v L / alpha [1]", "x / L [1]", [v for _, v in lines], V_MAX)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
