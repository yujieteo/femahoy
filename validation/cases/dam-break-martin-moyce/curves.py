"""The curves of the report of dam-break-martin-moyce (scripts/validation-reports.py).

    python3 curves.py OUT sph=FOLDER

FOLDER is the output folder of the run of sph. The script writes OUT/front.csv, the surge front
Z = x / a against T = t sqrt(2 g / a), with the measured values of derivation.md. The front is
v_Z of the thermo lines of lammps/log.lmp (every 1000 steps) and of the stops of `fix halt`. As in
case.lmp, a constant is added to T so that T is 1.19 at the first stop, Z = 1.44. Only the Python
standard library.
"""

import re
import sys
from pathlib import Path

# Martin and Moyce (1952), Table 2, a = 2.25 in, as in derivation.md: Z, the mean T of the runs,
# the smallest and the largest T of the single runs. T is 1.19 at Z = 1.44 in each run.
MEASURED = [(1.44, 1.19, None, None), (2.11, 1.83, 1.75, 1.88), (3.00, 2.51, 2.38, 2.66), (3.89, 3.11, 2.94, 3.25)]


def front(log):
    """(step, T, Z) of the thermo lines and of the stops, in the order of the steps. T is the
    T of LAMMPS, from t = 0."""
    points = {}
    ratio = None
    for line in log.read_text().splitlines():
        f = line.split()
        if len(f) == 4 and f[0].isdigit():
            step, _, t, z = int(f[0]), float(f[1]), float(f[2]), float(f[3])
            points[step] = (t, z)
            if step:
                ratio = t / step
        elif m := re.match(r"Fix halt condition for fix-id stop met on step (\d+) with value (\S+)", line):
            points[int(m[1])] = (None, float(m[2]))
    # A stop has no thermo line of its own: its T is the step times the T of 1 step.
    return [(s, t if t is not None else s * ratio, z) for s, (t, z) in sorted(points.items())]


def main(out, folders):
    run = Path(dict(f.split("=", 1) for f in folders)["sph"])
    rows = front(run / "lammps/log.lmp")
    t0 = next(t for _, t, z in rows if z >= 1.44)
    lines = [f"{t - t0 + 1.19:.4f},{z:.4f},,\n" for _, t, z in rows]
    for z, mean, low, high in MEASURED:
        lines.append(f"{mean},,{z},\n")
        lines += [f"{t},,,{z}\n" for t in (low, high) if t is not None]
    (Path(out) / "front.csv").write_text(
        "# title: Dam break, the surge front (LAMMPS SPH, 40 particles across a)\n"
        "# y: Z = x / a, the front [1]\n"
        "# points measured: Martin and Moyce (1952), Table 2, the mean of the runs with a = 2.25 in\n"
        "# points single runs: Martin and Moyce (1952), Table 2, the smallest and the largest T of the runs with a = 2.25 in\n"
        "# note: The front is the largest x of the water plus dx / 2. T is 1.19 at Z = 1.44, as in the source.\n"
        "T = t sqrt(2 g / a) [1],SPH,measured,single runs\n" + "".join(lines)
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
