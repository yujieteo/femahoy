"""The curve of the report of taylor-bar-impact (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/shape.csv: the length
of the bar and the mushroom radius against the time, from the *NODE PRINT of TOP and RIM in
ccx/job.dat, with the DYNA3D values at 80 us of check.toml. Only the Python standard library.
"""

import re
import sys
from pathlib import Path

LENGTH, RADIUS = 32.4, 3.2  # the bar at the start, mm
# The DYNA3D results at 80 us (check.toml): final length and mushroom radius, mm.
DYNA3D = (21.66, 7.02)


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    text = (Path(runs["job.inp"]) / "ccx/job.dat").read_text()
    blocks = re.findall(r"displacements \(vx,vy,vz\) for set (\w+) and time\s+(\S+)\n\n((?:\s+\d+(?:\s+\S+){3}\n)+)", text)
    shape = {0.0: [LENGTH, RADIUS]}
    for name, time, rows in blocks:
        u = [[float(v) for v in row.split()[1:]] for row in rows.splitlines()]
        values = shape.setdefault(float(time) * 1e6, [None, None])
        if name == "TOP":
            values[0] = LENGTH + sum(r[2] for r in u) / len(u)  # the mean u3 of the top face
        elif name == "RIM":
            values[1] = RADIUS + u[0][0]  # u1 of the rim node on the wall
    rows = [f"{t:g},{length:.4f},{radius:.4f}," for t, (length, radius) in sorted(shape.items())]
    rows += [f"80,,,{d}" for d in DYNA3D]
    (Path(out) / "shape.csv").write_text(
        "# title: The length of the bar and the mushroom radius against the time\n"
        "# y: length or radius [mm]\n"
        "# points DYNA3D: DYNA3D (Hallquist and Benson 1987), from the Ansys Technology Showcase, chapter 17\n"
        "# note: The length is 32.4 mm plus the mean u3 of the top face. The radius is 3.2 mm plus u1 of the rim node on the wall. "
        "ccx writes them each 10 µs.\n"
        "t [µs],length,radius,DYNA3D\n" + "\n".join(rows) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
