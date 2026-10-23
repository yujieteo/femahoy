"""Write global-mesh.inp and mesh.inp: the coarse global model and the fine submodel of 1/4 of a
plate with a circular hole, in plane strain. Print the reference value of the check.

Usage: python3 mesh.py [--global NS NT] [--sub NS NR] [--cut R] [--exact-cut FILE]

The hole has the radius A = 10 mm, with its centre at (0, 0). The global model is the square
0 <= x, y <= 50 mm minus the hole. 2 blocks (0 to 45 deg and 45 to 90 deg) of CPE8R elements go
from the hole to the outer edges: NS elements along the hole and NT elements to the edge in each
block. The outer edges (x = 50 and y = 50) get the displacement of the Kirsch solution.

The submodel is the ring A <= r <= R (the cut). It has NS elements along 90 deg of the hole and
NR elements from the hole to the cut, each 1.2 times longer than the one before. The submodel
gets the displacement of the global model on the cut (the node set CUT).

The reference is the Kirsch stress sxx at the integration points of the element TOP of the
submodel, at the hole next to (0, A). With --exact-cut, the script also writes the Kirsch
displacement of the nodes of CUT to FILE (see derivation.md).
"""

import argparse
import math

S, A, L = 100.0, 10.0, 50.0
E, NU = 210000.0, 0.3
MU, KAPPA = E / (2 * (1 + NU)), 3 - 4 * NU  # plane strain
GROWTH = 1.2

p = argparse.ArgumentParser()
p.add_argument("--global", dest="glob", type=int, nargs=2, default=(2, 4), metavar=("NS", "NT"))
p.add_argument("--sub", type=int, nargs=2, default=(24, 12), metavar=("NS", "NR"))
p.add_argument("--cut", type=float, default=20.0, help="radius R of the cut")
p.add_argument("--exact-cut", metavar="FILE")
args = p.parse_args()


def kirsch_u(x, y):
    """The displacement (ux, uy) of the Kirsch solution for the tension S along x."""
    r, t = math.hypot(x, y), math.atan2(y, x)
    c = S * A / (8 * MU)
    ux = c * (r / A * (KAPPA + 1) * math.cos(t) + 2 * A / r * ((1 + KAPPA) * math.cos(t) + math.cos(3 * t))
              - 2 * A**3 / r**3 * math.cos(3 * t))
    uy = c * (r / A * (KAPPA - 3) * math.sin(t) + 2 * A / r * ((1 - KAPPA) * math.sin(t) + math.sin(3 * t))
              - 2 * A**3 / r**3 * math.sin(3 * t))
    return ux, uy


def kirsch_sxx(x, y):
    r, t = math.hypot(x, y), math.atan2(y, x)
    q = A * A / (r * r)
    srr = S / 2 * (1 - q) + S / 2 * (1 - 4 * q + 3 * q * q) * math.cos(2 * t)
    stt = S / 2 * (1 + q) - S / 2 * (1 + 3 * q * q) * math.cos(2 * t)
    srt = -S / 2 * (1 + 2 * q - 3 * q * q) * math.sin(2 * t)
    c, s = math.cos(t), math.sin(t)
    return srr * c * c + stt * s * s - 2 * srt * s * c


def grading(n, growth):
    """The 2n + 1 node positions in [0, 1] of n elements, each `growth` times the one before."""
    h = [growth**k for k in range(n)]
    total, at, out = sum(h), 0.0, [0.0]
    for k in range(n):
        out += [(at + h[k] / 2) / total, (at + h[k]) / total]
        at += h[k]
    return out


class Mesh:
    def __init__(self):
        self.nodes, self.ids, self.elements = [], {}, []

    def node(self, x, y):
        key = (round(x, 9), round(y, 9))
        if key not in self.ids:
            self.nodes.append((x, y))
            self.ids[key] = len(self.nodes)
        return self.ids[key]

    def block(self, point, ns, ts):
        """The 8-node elements of a mapped block, as a list of element numbers. point(s, t) is the
        point of the block: s goes along the hole (0 to 1), t goes out from the hole (the node
        positions ts)."""
        ids = {}
        for i in range(2 * ns + 1):
            for j, t in enumerate(ts):
                if not (i % 2 and j % 2):
                    ids[i, j] = self.node(*point(i / (2 * ns), t))
        out = []
        for i in range(0, 2 * ns, 2):
            for j in range(0, len(ts) - 1, 2):
                self.elements.append([ids[i, j], ids[i, j + 2], ids[i + 2, j + 2], ids[i + 2, j],
                                      ids[i, j + 1], ids[i + 1, j + 2], ids[i + 2, j + 1], ids[i + 1, j]])
                out.append(len(self.elements))
        return out

    def where(self, test):
        return [i for i, (x, y) in enumerate(self.nodes, 1) if test(x, y)]


def write(path, mesh, title, sets, tail=""):
    with open(path, "w") as f:
        f.write(f"** {title} Written by mesh.py.\n*NODE\n")
        for i, (x, y) in enumerate(mesh.nodes, 1):
            f.write(f"{i}, {x:.10g}, {y:.10g}, 0.0\n")
        f.write("*ELEMENT, TYPE=CPE8R, ELSET=PLATE\n")
        for i, c in enumerate(mesh.elements, 1):
            f.write(f"{i}, " + ", ".join(map(str, c)) + "\n")
        for name, kind, ids in sets:
            f.write(f"*{kind}SET, {kind}SET={name}\n")
            for k in range(0, len(ids), 12):
                f.write(", ".join(map(str, ids[k:k + 12])) + ",\n")
        f.write(tail)


def boundary(mesh, ids):
    """*BOUNDARY with the Kirsch displacement of the nodes ids."""
    out = "*BOUNDARY\n"
    for i in ids:
        ux, uy = kirsch_u(*mesh.nodes[i - 1])
        out += f"{i}, 1, 1, {ux:.10e}\n{i}, 2, 2, {uy:.10e}\n"
    return out


def on(value):
    return lambda v: abs(v - value) < 1e-9


# The global model. Block 1 goes from the hole to the edge x = L, block 2 to the edge y = L.
ns, nt = args.glob
g = Mesh()


def hole(s, start):
    t = math.pi / 4 * (start + s)
    return A * math.cos(t), A * math.sin(t)


def block1(s, t):
    (hx, hy), (ex, ey) = hole(s, 0), (L, s * L)
    return hx + t * (ex - hx), hy + t * (ey - hy)


def block2(s, t):
    (hx, hy), (ex, ey) = hole(s, 1), (L * (1 - s), L)
    return hx + t * (ex - hx), hy + t * (ey - hy)


ts = grading(nt, 1.5)
g.block(block1, ns, ts)
g.block(block2, ns, ts)
edge = g.where(lambda x, y: on(L)(x) or on(L)(y))
write("global-mesh.inp", g, "Global model: 1/4 of the plate, the Kirsch displacement on the outer edges.",
      [("LEFT", "N", g.where(lambda x, y: on(0)(x) and not on(L)(y))),
       ("BOTTOM", "N", g.where(lambda x, y: on(0)(y) and not on(L)(x))),
       ("EDGE", "N", edge)],
      boundary(g, edge))

# The submodel.
ns, nr = args.sub
m = Mesh()
rs = [A + t * (args.cut - A) for t in grading(nr, GROWTH)]
ring = m.block(lambda s, r: (r * math.cos(math.pi / 2 * s), r * math.sin(math.pi / 2 * s)), ns, rs)
cut = m.where(lambda x, y: on(args.cut)(math.hypot(x, y)))
top = ring[(ns - 1) * nr]
write("mesh.inp", m, f"Submodel: the ring {A:g} <= r <= {args.cut:g} mm about the hole.",
      [("LEFT", "N", m.where(lambda x, y: on(0)(x) and not on(args.cut)(y))),
       ("BOTTOM", "N", m.where(lambda x, y: on(0)(y) and not on(args.cut)(x))),
       ("CUT", "N", cut),
       ("TOP", "EL", [top])])
if args.exact_cut:
    with open(args.exact_cut, "w") as f:
        f.write(boundary(m, cut))


def shape(xi, eta):
    """The 8 shape functions of a CPE8R element at (xi, eta), in the order of the nodes."""
    n = [(1 + a * xi) * (1 + b * eta) * (a * xi + b * eta - 1) / 4 for a, b in [(-1, -1), (1, -1), (1, 1), (-1, 1)]]
    return n + [(1 - xi * xi) * (1 - eta) / 2, (1 + xi) * (1 - eta * eta) / 2,
                (1 - xi * xi) * (1 + eta) / 2, (1 - xi) * (1 - eta * eta) / 2]


# CPE8R has 2 x 2 integration points at +-1/sqrt(3). ccx makes a C3D20R of each element, with 2
# points across the thickness at each point of the plane.
g3 = 1 / math.sqrt(3)
points = []
for xi in (-g3, g3):
    for eta in (-g3, g3):
        n = shape(xi, eta)
        corners = m.elements[top - 1]
        points.append((sum(w * m.nodes[k - 1][0] for w, k in zip(n, corners)),
                       sum(w * m.nodes[k - 1][1] for w, k in zip(n, corners))))
x, y = max(points, key=lambda p: kirsch_sxx(*p))
print(f"global-mesh.inp: {len(g.nodes)} nodes, {len(g.elements)} elements")
print(f"mesh.inp: {len(m.nodes)} nodes, {len(m.elements)} elements, TOP is element {top}")
print(f"sxx of Kirsch at the point of TOP nearest (0, {A:g}): ({x:.6f}, {y:.6f}): {kirsch_sxx(x, y):.6f} MPa")
