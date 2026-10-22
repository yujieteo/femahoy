"""Write mesh.inp: 1/4 of a thick tube in plane strain, with 8-node quadrilaterals.

Usage: python3 mesh.py [--radial N] [--hoop M] [-o FILE]

The tube has the inner radius A = 10 mm and the outer radius B = 20 mm. The part is the quarter
0 <= theta <= 90 degrees of the x-y plane. The elements are CPE8R, N through the wall and M about
the axis, all of the same size in r and theta. The mid-side nodes are on the arcs.
"""

import argparse
import math

A, B = 10.0, 20.0

p = argparse.ArgumentParser()
p.add_argument("--radial", type=int, default=8)
p.add_argument("--hoop", type=int, default=16)
p.add_argument("-o", default="mesh.inp")
args = p.parse_args()
n, m = args.radial, args.hoop


def node(i, j):
    """The number of the node in row i (0 at r = A, 2n at r = B) and column j (0 at theta = 0)."""
    return j * (2 * n + 1) + i + 1


nodes = []
for j in range(2 * m + 1):
    for i in range(2 * n + 1):
        if i % 2 and j % 2:
            continue
        r, t = A + (B - A) * i / (2 * n), 0.5 * math.pi * j / (2 * m)
        x = 0.0 if j == 2 * m else r * math.cos(t)
        y = 0.0 if j == 0 else r * math.sin(t)
        nodes.append((node(i, j), x, y))
elements = []
for je in range(m):
    for ie in range(n):
        i, j = 2 * ie, 2 * je
        corners = [node(i, j), node(i + 2, j), node(i + 2, j + 2), node(i, j + 2)]
        mids = [node(i + 1, j), node(i + 2, j + 1), node(i + 1, j + 2), node(i, j + 1)]
        elements.append(corners + mids)

inner = [node(0, j) for j in range(2 * m + 1)]
# The inner nodes have their own supports in job.inp, so the symmetry sets leave them out.
sy = [node(i, 0) for i in range(1, 2 * n + 1)]
sx = [node(i, 2 * m) for i in range(1, 2 * n + 1)]


def rows(ids, k=8):
    return "\n".join(", ".join(map(str, ids[s : s + k])) for s in range(0, len(ids), k))


with open(args.o, "w") as f:
    f.write(f"** mesh.py --radial {n} --hoop {m}\n")
    f.write("*NODE, NSET=NALL\n")
    f.writelines(f"{k}, {x:.9g}, {y:.9g}\n" for k, x, y in nodes)
    f.write("*ELEMENT, TYPE=CPE8R, ELSET=ALL\n")
    f.writelines(f"{e}, {', '.join(map(str, q))}\n" for e, q in enumerate(elements, 1))
    f.write(f"*NSET, NSET=INNER\n{rows(inner)}\n")
    f.write(f"*NSET, NSET=SX\n{rows(sx)}\n")
    f.write(f"*NSET, NSET=SY\n{rows(sy)}\n")
    # The x reactions of these nodes are the pressure force on the inner face. The node on x = 0
    # is not in them: its x reaction also holds the hoop force of the symmetry plane.
    for name in ("F25", "F40"):
        f.write(f"*NSET, NSET={name}\n{rows(inner[:-1])}\n")
    f.write(f"*NSET, NSET=PB\n{node(2 * n, 0)}\n")

print(f"{len(nodes)} nodes, {len(elements)} CPE8R")
