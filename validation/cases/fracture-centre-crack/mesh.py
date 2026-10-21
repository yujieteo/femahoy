"""Write mesh.inp: 1/4 of a plate with a centre crack, in plane strain, with a spider web mesh.

Usage: python3 mesh.py [--crack A] [--sectors N] [--rings N1 N2] [--tip L] [--height H] [-o FILE]

The plate is 0 <= x <= 50, 0 <= y <= H. The crack face is y = 0, x < A. The tip is (A, 0).
Rays from the tip go to the edges of the plate. Ring 1 has CPE6 triangles. In each triangle, the
mid-side nodes of the 2 edges from the tip are at 1/4 of the edge (quarter-point elements). The
other rings have CPE8 quadrilaterals. The file ends with the equation that makes U2 of the node
set KI equal to K_I (see derivation.md).
"""

import argparse
import math

B = 50.0
E, NU = 210000.0, 0.3

p = argparse.ArgumentParser()
p.add_argument("--crack", type=float, default=25.0, help="half crack length A")
p.add_argument("--sectors", type=int, default=16)
p.add_argument("--rings", type=int, nargs=2, default=(8, 8))
p.add_argument("--tip", type=float, help="size L of the ring 1 elements, A / 50 by default")
p.add_argument("--height", type=float, default=150.0)
p.add_argument("-o", default="mesh.inp")
args = p.parse_args()
A, H, N = args.crack, args.height, args.sectors
L = args.tip or A / 50.0
n1, n2 = args.rings
R0 = min(A, B - A) / 5.0

# The rays to the corners (B, H) and (0, H) are rays of the mesh, so that each sector ends on 1
# edge of the plate.
c1, c2 = math.atan2(H, B - A), math.atan2(H, -A)
j1 = max(1, round(N * c1 / math.pi))
j2 = min(N - 1, max(j1 + 1, round(N * c2 / math.pi)))


def angle(j):
    if j <= j1:
        return c1 * j / j1
    if j <= j2:
        return c1 + (c2 - c1) * (j - j1) / (j2 - j1)
    return c2 + (math.pi - c2) * (j - j2) / (N - j2)


def edge(t):
    """The distance from the tip to the edge of the plate along the ray at angle t."""
    c, s = math.cos(t), math.sin(t)
    d = [H / s] if s > 1e-12 else []
    d += [(B - A) / c] if c > 1e-12 else []
    d += [-A / c] if c < -1e-12 else []
    return min(d)


def radius(k, j):
    """The distance of ring k (0 is the tip) from the tip on ray j."""
    if k <= n1:
        return 0.0 if k == 0 else L * (R0 / L) ** ((k - 1) / (n1 - 1))
    return R0 * (edge(angle(j)) / R0) ** ((k - n1) / n2)


nodes, ids = [], {}


def node(key, x, y):
    if key not in ids:
        nodes.append((0.0 if abs(x) < 1e-9 else x, 0.0 if abs(y) < 1e-9 else y))
        ids[key] = len(nodes)
    return ids[key]


def corner(k, j):
    r, t = radius(k, j), angle(j)
    return node(("c", k, 0 if k == 0 else j), A + r * math.cos(t), r * math.sin(t))


def mid(n, m, f=0.5):
    """The node at the fraction f of the straight edge from node n to node m."""
    (x0, y0), (x1, y1) = nodes[n - 1], nodes[m - 1]
    key = ("m", n, m) if f != 0.5 else ("m",) + tuple(sorted((n, m)))
    return node(key, x0 + f * (x1 - x0), y0 + f * (y1 - y0))


tip = corner(0, 0)
tris = []
for j in range(N):
    a, b = corner(1, j), corner(1, j + 1)
    tris.append((tip, a, b, mid(tip, a, 0.25), mid(a, b), mid(tip, b, 0.25)))
quads = []
for k in range(1, n1 + n2):
    for j in range(N):
        c = [corner(k, j), corner(k + 1, j), corner(k + 1, j + 1), corner(k, j + 1)]
        quads.append((k, j, c + [mid(c[i], c[(i + 1) % 4]) for i in range(4)]))

ligament = [i for i, (x, y) in enumerate(nodes, 1) if y == 0.0 and x >= A]
left = [i for i, (x, y) in enumerate(nodes, 1) if x == 0.0]
# Face 2 (nodes 2-3) of the elements of the last ring in the sectors j1 to j2 is on y = H.
top = [len(tris) + i for i, (k, j, _) in enumerate(quads, 1) if k == n1 + n2 - 1 and j1 <= j < j2]
# On the crack face: the corner QC at the distance L from the tip, and the quarter-point node QB.
qc = corner(1, N)
qb = mid(tip, qc, 0.25)
ki = len(nodes) + 1
c = E / (1 - NU**2) / 4.0 * math.sqrt(2.0 * math.pi / L)


def rows(ids_, n=8):
    return "\n".join(", ".join(map(str, ids_[s : s + n])) for s in range(0, len(ids_), n))


with open(args.o, "w") as f:
    f.write(f"** mesh.py --crack {A} --sectors {N} --rings {n1} {n2} --tip {L} --height {H}\n")
    f.write("*NODE, NSET=NALL\n")
    f.writelines(f"{i}, {x:.9g}, {y:.9g}\n" for i, (x, y) in enumerate(nodes, 1))
    f.write("*ELEMENT, TYPE=CPE6, ELSET=PLATE\n")
    f.writelines(f"{i}, {', '.join(map(str, t))}\n" for i, t in enumerate(tris, 1))
    f.write("*ELEMENT, TYPE=CPE8, ELSET=PLATE\n")
    first = len(tris) + 1
    f.writelines(f"{i}, {', '.join(map(str, q))}\n" for i, (_, _, q) in enumerate(quads, first))
    f.write(f"*NSET, NSET=LIGAMENT\n{rows(ligament)}\n")
    f.write(f"*NSET, NSET=LEFT\n{rows(left)}\n")
    f.write(f"*ELSET, ELSET=TOP\n{rows(top)}\n")
    f.write(
        f"** K_I = c (4 v_QB - v_QC), c = E / (1 - nu^2) / 4 * sqrt(2 pi / L), L = {L}.\n"
        f"** QB = {qb}, QC = {qc}. The node {ki} has no element. Its U2 is K_I.\n"
    )
    f.write(f"*NODE, NSET=KI\n{ki}, {A}, -{L}\n")
    f.write(f"*EQUATION\n3\n{ki}, 2, 1.0, {qb}, 2, {-4 * c:.9g}, {qc}, 2, {c:.9g}\n")

print(f"{len(nodes)} nodes, {len(tris)} CPE6, {len(quads)} CPE8")
