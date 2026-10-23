"""The curves of the report of wedge-oblique-shock (scripts/validation-reports.py).

    python3 curves.py OUT mach2.5=FOLDER

FOLDER is the output folder of the run of the input. The script writes 2 curves on the column
of cells at x = 0.755, from the wedge to the far field: OUT/pressure.csv, the pressure with the
exact oblique shock, and OUT/behind.csv, the state behind the shock divided by the exact state.
Only the Python standard library.
"""

import math
import re
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import vtu  # noqa: E402

X = 0.755  # the centre of a column of cells, 0.005 after the line x = 0.75 of derivation.md
WEDGE = 15.0
# Table 2 of the NPARC case (check.toml): the shock angle and the state behind the shock.
CHECKS = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
BETA, P2, T2, M2 = (CHECKS[k]["value"] for k in ("shock_angle", "pressure_ratio", "temperature_ratio", "mach2"))


def column(folder):
    """(y, p, T, Ma) of the cells of the column at x = X, from the wedge up."""
    steps = sorted(folder.glob("step-*.vtu"), key=lambda p: int(re.search(r"\d+", p.stem)[0]))
    step = vtu.read(steps[-1])
    cells = []
    for k, p in enumerate(step.cell["p"]):
        nodes = step.nodes(k)
        x = sum(step.points[i][0] for i in nodes) / len(nodes)
        if abs(x - X) < 1e-4:
            y = sum(step.points[i][1] for i in nodes) / len(nodes)
            cells.append((y, p, step.cell["T"][k], step.cell["Ma"][k]))
    return sorted(cells)


def main(out, folders):
    out = Path(out)
    runs = dict(f.split("=", 1) for f in folders)
    cells = column(Path(runs["mach2.5"]))
    wall = X * math.tan(math.radians(WEDGE))
    shock = X * math.tan(math.radians(BETA))
    rows = [f"{y:.5f},{p:.5f}," for y, p, _, _ in cells]
    rows += [f"{y:.5f},,{p}" for y, p in [(wall, P2), (shock, P2), (shock, 1), (1, 1)]]
    (out / "pressure.csv").write_text(
        f"# title: The pressure on the line x = {X}, from the wedge to the far field\n"
        "# y: p / p1 [1]\n"
        "# note: The theory is the exact oblique shock of Table 2 of the NPARC case: the shock at "
        f"y = {shock:.4f} (beta = {BETA} degrees) and p2 / p1 = {P2}.\n"
        "y [1],model,theory\n" + "\n".join(rows) + "\n"
    )
    # Behind the shock: the cells below the shock with the full pressure rise.
    behind = [c for c in cells if c[0] < shock and c[1] > 0.99 * P2]
    rows = [f"{y:.5f},{p / P2:.5f},{t / T2:.5f},{m / M2:.5f}," for y, p, t, m in behind]
    rows += [f"{wall:.5f},,,,1", f"{behind[-1][0]:.5f},,,,1"]
    (out / "behind.csv").write_text(
        f"# title: The state behind the shock on the line x = {X}, divided by the theory\n"
        "# y: model / theory [1]\n"
        "# note: The checks mach2, temperature_ratio and density2 average the cells with 0.25 < x "
        f"and y < x / 2: here y < {X / 2:.4f}.\n"
        f"# note: The wedge is at y = {wall:.4f}.\n"
        "y [1],p / p2,T / T2,Ma / M2,theory\n" + "\n".join(rows) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
