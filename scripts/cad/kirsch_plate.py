"""Mesh script for validation/cases/kirsch-plate-c3d10: a quarter plate with a hole, C3D10.

Usage: python kirsch_plate.py OUT_DIR
Python 3 only, no package needed. Units are mm, N, MPa.

The quarter plate is a square of half-width W with a hole of radius A at the origin.
Two blocks (angle 0 to 45 degrees, and 45 to 90 degrees) map a grid (angle, radius) to the plate.
Each grid cell is a hex, one cell thick in z. Each hex gives 24 quadratic tetrahedra.
Each midside node sits at the map of the parameter midpoint, so the hole is a true circle.
The load axis is x. The plate carries a tension P on the face x = W.
"""
import math
import sys

A, W, P = 10.0, 200.0, 100.0  # hole radius, half-width (mm), far-field stress (MPa)
E, NU = 210000.0, 0.3
GRADE = 2.5  # radial grading: the cells grow with distance from the hole
MESHES = [(4, 4), (8, 8), (16, 16)]  # (cells over 90 degrees, cells in radius)
EPS = 1e-9


def grade(x):
    return (math.exp(GRADE * x) - 1.0) / (math.exp(GRADE) - 1.0)


def build(nt, nr):
    half = nt // 2
    thick = (math.pi / 2 * A) / nt  # the cell size on the hole, so no tet is flat

    def xyz(q):  # q is (radial, angle, z) in grid units, scaled by 4
        s, u, k = (c / 4.0 for c in q)
        th = (math.pi / 2) * u / nt
        if u <= half:
            outer = (W, W * math.tan(th))
        else:
            outer = (W / math.tan(th), W)
        g = grade(s / nr)
        inner = (A * math.cos(th), A * math.sin(th))
        return (inner[0] + g * (outer[0] - inner[0]), inner[1] + g * (outer[1] - inner[1]), k * thick)

    ids, coords = {}, []

    def node(q):
        if q not in ids:
            ids[q] = len(coords) + 1
            coords.append(xyz(q))
        return ids[q]

    def mid(a, b):
        assert all((x + y) % 2 == 0 for x, y in zip(a, b))
        return tuple((x + y) // 2 for x, y in zip(a, b))

    faces = [  # corner offsets of the 6 faces of a hex, counter-clockwise from outside
        [(0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)],
        [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)],
        [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)],
        [(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)],
        [(1, 1, 0), (0, 1, 0), (0, 1, 1), (1, 1, 1)],
        [(0, 1, 0), (0, 0, 0), (0, 0, 1), (0, 1, 1)],
    ]
    tets = []  # (corner keys, cell index, face index)
    for i in range(nt):
        for j in range(nr):
            def key(o):
                return (4 * (j + o[0]), 4 * (i + o[1]), 4 * o[2])
            h = (4 * j + 2, 4 * i + 2, 2)
            for fi, f in enumerate(faces):
                c = [key(o) for o in f]
                fc = tuple(sum(x[d] for x in c) // 4 for d in range(3))
                for t in range(4):
                    tets.append(((c[t], c[(t + 1) % 4], fc, h), (i, j), fi))

    def vol(p):
        a = [tuple(xyz(q)[d] - xyz(p[0])[d] for d in range(3)) for q in p[1:]]
        return (
            a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
        )

    elems = []
    for q, cell, fi in tets:
        q = list(q)
        v = vol(q)
        assert abs(v) > 1e-12, "flat tetrahedron"
        if v < 0:
            q[1], q[2] = q[2], q[1]
        n = [node(x) for x in q]
        m = [node(mid(q[a], q[b])) for a, b in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)]]
        elems.append((n + m, cell, fi, q))
    return coords, elems, nt, nr


def write(path, coords, elems, nt, nr):
    out = ["*NODE"]
    for i, c in enumerate(coords, 1):
        out.append(f"{i}, {c[0]:.12g}, {c[1]:.12g}, {c[2]:.12g}")
    out.append("*ELEMENT, TYPE=C3D10")
    for e, (nodes, _, _, _) in enumerate(elems, 1):
        out.append(f"{e}, " + ", ".join(map(str, nodes[:5])) + ",")
        out.append("  " + ", ".join(map(str, nodes[5:])))

    def lines(name, kind, ids):
        out.append(f"*{kind}, {kind}={name}")
        out.extend(f"{i}," for i in ids)

    lines("plate", "ELSET", range(1, len(elems) + 1))
    # The elements on the hole in the cell next to theta = 90 degrees and theta = 0.
    hole = lambda col: [e for e, (_, c, f, _) in enumerate(elems, 1) if c == (col, 0) and f == 5]
    # Hole face: the hex face with radial index 0 is face 5 (index 5), the 4 tets of that face.
    lines("edge90", "ELSET", hole(nt - 1))
    lines("edge0", "ELSET", hole(0))
    lines("xplane", "NSET", [i for i, c in enumerate(coords, 1) if abs(c[0]) < EPS])
    lines("yplane", "NSET", [i for i, c in enumerate(coords, 1) if abs(c[1]) < EPS])
    lines("allnodes", "NSET", range(1, len(coords) + 1))
    out += [
        "*material, name=steel", "*elastic", f"{E}, {NU}",
        "*solid section, elset=plate, material=steel",
        "*step, name=tension", "*static", "1.0, 1.0, 1e-4, 1.0",
        "*boundary", "xplane, 1, 1", "yplane, 2, 2", "allnodes, 3, 3",
        "*DLOAD",
    ]
    tf = [(0, 1, 2), (0, 3, 1), (1, 3, 2), (2, 3, 0)]  # C3D10 faces, 0 based corners
    for e, (_, _, _, q) in enumerate(elems, 1):
        for k, f in enumerate(tf, 1):
            if all(abs(_xyz(coords, elems, e, a)[0] - W) < EPS for a in f):
                out.append(f"{e}, P{k}, {-P}")
    out.append("*end step")
    with open(path, "w") as fh:
        fh.write("\n".join(out) + "\n")


def _xyz(coords, elems, e, a):
    return coords[elems[e - 1][0][a] - 1]


if __name__ == "__main__":
    for nt, nr in MESHES:
        write(f"{sys.argv[1]}/mesh-{nt}x{nr}.inp", *build(nt, nr))
