"""Write mesh.inp of the case: a quarter of the bar, radius 3.2 mm, length 32.4 mm, in hexahedra.

The section is an O-grid: a square of N x N elements about the axis and a ring of M elements to
the outer face. The bar has NZ layers in z. The impact face is z = 0. The top layer is G times as
thick as the layer at the impact face, because the deformation is near the impact face.

    python3 mesh.py [N M NZ G]    defaults: 6 6 40 5
"""

import math
import sys

R, L = 3.2, 32.4
N, M, NZ = (int(a) for a in sys.argv[1:4]) if len(sys.argv) > 3 else (6, 6, 40)
G = float(sys.argv[4]) if len(sys.argv) > 4 else 5.0
Q = G ** (1 / (NZ - 1))  # the ratio of 2 adjacent layers
Z = [L * k / NZ if G == 1 else L * (Q**k - 1) / (Q**NZ - 1) for k in range(NZ + 1)]
S = 0.5 * R  # half side of the square about the axis


def point(block, u, v):
    """The point (x, y) of the section at (u, v) in [0, 1] of a block."""
    if block == 0:  # the square
        return S * u, S * v
    if block == 1:  # the ring, 0 to 45 degrees
        inner, t = (S, S * v), v * math.pi / 4
    else:  # the ring, 45 to 90 degrees
        inner, t = (S * (1 - v), S), (1 + v) * math.pi / 4
    outer = (R * math.cos(t), R * math.sin(t))
    return tuple((1 - u) * a + u * b for a, b in zip(inner, outer))


ids, xy, quads = {}, [], []


def node(p):
    key = (round(p[0], 9), round(p[1], 9))
    if key not in ids:
        ids[key] = len(xy)
        xy.append(p)
    return ids[key]


for block, (nu, nv) in enumerate([(N, N), (M, N), (M, N)]):
    for i in range(nu):
        for j in range(nv):
            corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
            q = [node(point(block, a / nu, b / nv)) for a, b in corners]
            (x0, y0), (x1, y1), (x2, y2) = (xy[k] for k in q[:3])
            if (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0) < 0:
                q.reverse()
            quads.append(q)

n2 = len(xy)
with open("mesh.inp", "w") as f:
    f.write(f"** mesh.py {N} {M} {NZ} {G:g}: {N} x {N} + 2 x {M} x {N} hexahedra in the section\n")
    f.write("*NODE, NSET=NALL\n")
    for k in range(NZ + 1):
        for i, (x, y) in enumerate(xy):
            f.write(f"{k * n2 + i + 1}, {x:.9g}, {y:.9g}, {Z[k]:.9g}\n")
    f.write("*ELEMENT, TYPE=C3D8R, ELSET=BAR\n")
    for k in range(NZ):
        for e, q in enumerate(quads):
            nodes = [k * n2 + i + 1 for i in q] + [(k + 1) * n2 + i + 1 for i in q]
            f.write(f"{k * len(quads) + e + 1}, {', '.join(map(str, nodes))}\n")

    def nset(name, members):
        f.write(f"*NSET, NSET={name}\n")
        for a in range(0, len(members), 16):
            f.write(", ".join(map(str, members[a : a + 16])) + "\n")

    every = [(k, i) for k in range(NZ + 1) for i in range(n2)]
    nset("SYMX", [k * n2 + i + 1 for k, i in every if abs(xy[i][0]) < 1e-9])
    nset("SYMY", [k * n2 + i + 1 for k, i in every if abs(xy[i][1]) < 1e-9])
    nset("WALL", [i + 1 for i in range(n2)])
    nset("TOP", [NZ * n2 + i + 1 for i in range(n2)])
    rim = [i for i in range(n2) if abs(xy[i][1]) < 1e-9 and abs(xy[i][0] - R) < 1e-9]
    nset("RIM", [i + 1 for i in rim])
