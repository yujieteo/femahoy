"""Print the reference values of check.toml. With RUN, write traction.csv from a run of job.inp.

Usage: python3 cattaneo.py [RUN]

2 elastic cylinders of the same material in plane strain (here a cylinder of radius R on a flat),
with the normal force P and then the tangential force Q < mu P, each for 1 mm of length:
  Hertz:    a = sqrt(4 P R / (pi E*)), p(x) = p0 sqrt(1 - x^2/a^2), p0 = 2 P / (pi a),
            1/E* = 2 (1 - nu^2) / E.
  Cattaneo: c = a sqrt(1 - Q / (mu P)). The centre |x| < c sticks, c < |x| < a slips.
            q(x) = mu p(x) - mu p0 (c/a) sqrt(1 - x^2/c^2) in |x| < c, q(x) = mu p(x) outside.
            q(0) = mu p0 (1 - c/a).

RUN is a folder with job.frd and mesh.inp of a run of job.inp. The script reads the contact
pressure CPRESS and the shear traction CSHEAR1 at the nodes of the pad that touch the block, at
the end of step 3, and writes traction.csv with the values of Hertz and Cattaneo.
"""

import math
import sys
from pathlib import Path

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


print(f"E* = {E_STAR:.4f} MPa, Q = {Q} N")
print(f"a = {A:.6f} mm, p0 = {P0:.4f} MPa")
print(f"c = {C:.6f} mm, c/a = {C / A:.6f}, q(0) = {cattaneo(0.0):.4f} MPa")
print(f"full stick, no slip: q(0) = Q / (pi a) = {Q / (math.pi * A):.4f} MPa")

if len(sys.argv) > 1:
    run = Path(sys.argv[1])
    xy, part = {}, None
    for line in (run / "mesh.inp").read_text().splitlines():
        if line.startswith("*"):
            part = line.split(",")[0].strip().upper()
        elif part == "*NODE":
            n, x, y = line.split(",")
            xy[int(n)] = (float(x), float(y))
    lines = (run / "job.frd").read_text().splitlines()
    start = max(i for i, l in enumerate(lines) if l.startswith(" -4  CONTACT"))
    nodes = []
    for line in lines[start + 1 :]:
        if line.startswith(" -3"):
            break
        if line.startswith(" -1"):
            v = [float(line[13 + 12 * k : 25 + 12 * k]) for k in range(6)]
            if v[3] > 0:  # CPRESS > 0: the node touches the block
                nodes.append((xy[int(line[3:13])][0], v[3], v[4]))
    rows = [(x / A, f"{p / P0:.5f}", "", f"{q / (MU * P0):.5f}", "") for x, p, q in nodes]
    # Steps of a / 100, and the ends of the stick zone and of the contact, where q has a kink.
    grid = sorted({k / 100 * A for k in range(-120, 121)} | {-C, C, -A, A})
    rows += [(x / A, "", f"{hertz(x) / P0:.5f}", "", f"{cattaneo(x) / (MU * P0):.5f}") for x in grid]
    rows.sort(key=lambda r: r[0])
    out = [
        "# title: Contact tractions at the end of step 3",
        "# y: p / p0 and q / (mu p0) [1]",
        "# points ccx p: this case, CPRESS at the nodes of the pad",
        "# points ccx q: this case, CSHEAR1 at the nodes of the pad",
        f"# note: The centre |x| < c = {C / A:.2f} a sticks: q is less than mu p. The edges slip: q = mu p.",
        "# note: Written by cattaneo.py from job.frd of the run.",
        "x / a [1],ccx p,Hertz p,ccx q,Cattaneo q",
    ]
    out += [f"{x:.5f}," + ",".join(r) for x, *r in rows]
    Path("traction.csv").write_text("\n".join(out) + "\n")
    print(f"traction.csv: {len(nodes)} nodes of the pad in contact")
