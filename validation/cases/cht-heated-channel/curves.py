"""The curve of the report of cht-heated-channel (scripts/validation-reports.py).

    python3 curves.py OUT channel=FOLDER

FOLDER is the output folder of the run of the input. The script writes OUT/profile.csv, the
temperature across the half channel and the wall at the station x0 = 40 mm, with the exact
solution of derivation.md. Only the Python standard library.
"""

import re
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import vtu  # noqa: E402

CHECKS = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
# The exact temperatures of the interface and of the heated face at x0 (check.toml).
T_WALL, T_HEATER = CHECKS["interface_T"]["value"], CHECKS["heater_T"]["value"]
# The model of derivation.md, "Model", in SI units: the heat flux, the conductivity of the air,
# the half channel (the wall is as thick), the station and the length of a cell along x.
Q, K_F, B = 120.0, 0.025, 0.001
X0, CELL = 0.040, 0.0002


def exact(y):
    """The fully developed temperature at x0. In the air, the integral of
    k d2T/dy2 = rho cp u G with u = (3/2) U (1 - y^2/b^2) and dT/dy = 0 on the centre line. In
    the wall, the linear conduction of the heat flux q."""
    if y <= B:
        return T_WALL - 3 * Q / (2 * K_F * B) * (5 * B**2 / 12 - y**2 / 2 + y**4 / (12 * B**2))
    return T_WALL + (T_HEATER - T_WALL) * (y - B) / B


def model(folder):
    """(y, T) at x0: the mean of the 2 cell columns on each side of x0. T is linear in x there."""
    steps = sorted(folder.glob("step-*.vtu"), key=lambda p: int(re.search(r"\d+", p.stem)[0]))
    step = vtu.read(steps[-1])
    rows = {}
    for k, t in enumerate(step.cell["T"]):
        nodes = step.nodes(k)
        x = sum(step.points[i][0] for i in nodes) / len(nodes)
        y = sum(step.points[i][1] for i in nodes) / len(nodes)
        if abs(abs(x - X0) - CELL / 2) < CELL / 10:
            rows.setdefault(round(y, 9), []).append(t)
    return [(y, sum(v) / len(v)) for y, v in sorted(rows.items())]


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    rows = [f"{1000 * y:.4f},{t:.4f}," for y, t in model(Path(runs["channel"]))]
    rows += [f"{y / 20:.4f},,{exact(y / 20000):.4f}" for y in range(41)]
    (Path(out) / "profile.csv").write_text(
        "# title: The temperature across the half channel and the wall at x = 40 mm\n"
        "# y: T [K]\n"
        "# note: The air is at 0 < y < 1 mm and the wall at 1 < y < 2 mm. The heater is at y = 2 mm, "
        "the centre line at y = 0.\n"
        "# note: The exact line is the fully developed solution of derivation.md: the profile of "
        "Shah and London (1978) in the air, and a linear temperature in the wall.\n"
        "# note: The model is the mean of the 2 cell columns at x = 39.9 and 40.1 mm.\n"
        "y [mm],model,exact\n" + "\n".join(rows) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
