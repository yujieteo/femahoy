"""The curve of the report of contact-cattaneo (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/traction.csv: the contact
pressure CPRESS and the shear traction CSHEAR1 at the nodes of the pad that touch the block, at
the end of step 3 (ccx/job.frd), with the curves of Hertz and Cattaneo (1938):

  2 elastic cylinders of the same material in plane strain (here a cylinder of radius R on a
  flat), with the normal force P and then the tangential force Q < mu P, each for 1 mm of length.
  Hertz:    a = sqrt(4 P R / (pi E*)), p(x) = p0 sqrt(1 - x^2/a^2), p0 = 2 P / (pi a),
            1/E* = 2 (1 - nu^2) / E.
  Cattaneo: c = a sqrt(1 - Q / (mu P)). The centre |x| < c sticks, c < |x| < a slips.
            q(x) = mu p(x) - mu p0 (c/a) sqrt(1 - x^2/c^2) in |x| < c, q(x) = mu p(x) outside.
            q(0) = mu p0 (1 - c/a).

The script stops when check.toml gives other values of p0 and q(0). Only the Python standard
library.
"""

import math
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
E, NU, R, MU = 210000.0, 0.3, 20.0, 0.3
P, Q = 4500.0, 1012.5  # N, Q = 0.75 mu P
E_STAR = E / (2 * (1 - NU * NU))
A = math.sqrt(4 * P * R / (math.pi * E_STAR))
P0 = 2 * P / (math.pi * A)
C = A * math.sqrt(1 - Q / (MU * P))


def hertz(x):
    return P0 * math.sqrt(max(0.0, 1 - x * x / (A * A)))


def cattaneo(x):
    stick = MU * P0 * C / A * math.sqrt(1 - x * x / (C * C)) if abs(x) < C else 0.0
    return MU * hertz(x) - stick


def block(lines, start):
    """The values of each node in the frd block after the line start, up to its end line -3."""
    out = {}
    for line in lines[start + 1 :]:
        if line.startswith(" -3"):
            break
        if line.startswith(" -1"):
            out[int(line[3:13])] = [float(line[13 + 12 * k : 25 + 12 * k]) for k in range((len(line) - 13) // 12)]
    return out


def main(out, folders):
    checks = tomllib.loads((HERE / "check.toml").read_text())
    for name, value in [("p0", P0), ("q_centre", cattaneo(0.0))]:
        if abs(value / checks[name]["value"] - 1) > 1e-5:
            sys.exit(f"curves.py: the theory gives {name} = {value:.6g}, check.toml gives {checks[name]['value']}")
    lines = (Path(dict(f.split("=", 1) for f in folders)["job.inp"]) / "ccx/job.frd").read_text().splitlines()
    xyz = block(lines, next(i for i, l in enumerate(lines) if l.startswith("    2C")))
    contact = block(lines, max(i for i, l in enumerate(lines) if l.startswith(" -4  CONTACT")))
    # CPRESS > 0: the node touches the block.
    nodes = [(xyz[n][0], v[3], v[4]) for n, v in contact.items() if v[3] > 0]
    if not nodes:
        sys.exit("curves.py: job.frd has no node in contact")
    rows = [(x / A, f"{p / P0:.5f}", "", f"{q / (MU * P0):.5f}", "") for x, p, q in nodes]
    # Steps of a / 100, and the ends of the stick zone and of the contact, where q has a kink.
    grid = sorted({k / 100 * A for k in range(-120, 121)} | {-C, C, -A, A})
    rows += [(x / A, "", f"{hertz(x) / P0:.5f}", "", f"{cattaneo(x) / (MU * P0):.5f}") for x in grid]
    rows.sort(key=lambda r: r[0])
    (Path(out) / "traction.csv").write_text(
        "# title: Contact tractions at the end of step 3\n"
        "# y: p / p0 and q / (mu p0) [1]\n"
        "# points ccx p: this case, CPRESS at the nodes of the pad, ccx/job.frd of the run\n"
        "# points ccx q: this case, CSHEAR1 at the nodes of the pad, ccx/job.frd of the run\n"
        f"# note: The centre |x| < c = {C / A:.2f} a sticks: q is less than mu p. The edges slip: q = mu p.\n"
        "x / a [1],ccx p,Hertz p,ccx q,Cattaneo q\n"
        + "".join(f"{x:.5f}," + ",".join(r) + "\n" for x, *r in rows)
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
