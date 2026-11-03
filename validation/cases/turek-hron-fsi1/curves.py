"""The curve of the report of turek-hron-fsi1 (scripts/validation-reports.py).

    python3 curves.py OUT fsi1=FOLDER

FOLDER is the output folder of the run of the input. The script writes OUT/history.csv: the 4
checked values at the end of each time window of the coupling, divided by the reference of
check.toml. Only the Python standard library.
"""

import sys
import tomllib
from pathlib import Path

CHECKS = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
NAMES = [("drag", "drag"), ("lift", "lift"), ("ux_a", "ux of A"), ("uy_a", "uy of A")]


def table(path, header):
    """The rows of numbers of a table, as dicts of the columns of the header line."""
    lines = path.read_text().splitlines()
    names = header(lines)
    numbers = [ln for ln in lines if ln.split() and ln.split()[0][0] in "0123456789-."]
    return [dict(zip(names, map(float, ln.split()))) for ln in numbers]


def history(precice):
    """{window: {check: value}} at the end of each window. force.dat has 1 row for each iteration
    of the coupling: the last row of a window is its result."""
    windows = {}
    forces = table(precice / "fluid/postProcessing/forces/0/force.dat",
                   lambda lines: [ln for ln in lines if ln.startswith("#")][-1][1:].split())
    for row in forces:
        windows.setdefault(round(row["Time"]), {}).update(drag=row["total_x"], lift=row["total_y"])
    point = table(precice / "solid/precice-Solid-watchpoint-A.log", lambda lines: lines[0].split())
    for row in point:
        if round(row["Time"]) in windows:
            windows[round(row["Time"])].update(ux_a=row["Displacement0"], uy_a=row["Displacement1"])
    return windows


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    windows = history(Path(runs["fsi1"]) / "precice")
    rows = []
    for window, values in sorted(windows.items()):
        rows.append(f"{window}," + ",".join(f"{values[k] / CHECKS[k]['value']:.5f}" for k, _ in NAMES) + ",")
    rows.append(f"{max(windows)}," + "," * len(NAMES) + "1")
    (Path(out) / "history.csv").write_text(
        "# title: The checked values at the end of each time window, divided by the reference\n"
        "# y: model / reference [1]\n"
        "# points reference: Turek and Hron (2006), the results of FSI1\n"
        "# note: The inlet velocity increases from 0 to its full value in the first 2 windows. "
        "The check reads the values of window 30.\n"
        "time window [1]," + ",".join(label for _, label in NAMES) + ",reference\n" + "\n".join(rows) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
