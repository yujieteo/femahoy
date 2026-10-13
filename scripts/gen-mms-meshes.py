#!/usr/bin/env python3
"""Write the meshes of the class R cases validation/cases/mms-*/ (unit cube, jittered lattice).

The lattice, the jitter and the 6-tet split match the helpers of
crates/core-solver/tests/common/mod.rs. Run: python3 scripts/gen-mms-meshes.py
"""
import itertools
import math
import pathlib

JITTER = 0.08
WEIGHT = (1.0, 0.7, -0.5)
ROOT = pathlib.Path(__file__).resolve().parent.parent / "validation" / "cases"
CASES = {
    "mms-c3d8": ("C3D8", "hex", (2, 4, 8)),
    "mms-c3d8i": ("C3D8I", "hex", (2, 4, 8)),
    "mms-c3d10": ("C3D10", "tet", (1, 2, 4)),
}


def lattice(m):
    pts = []
    for k, j, i in itertools.product(range(m + 1), repeat=3):
        t = (i / m, j / m, k / m)
        bump = math.sin(math.pi * t[0]) * math.sin(math.pi * t[1]) * math.sin(math.pi * t[2])
        pts.append([t[d] + JITTER * WEIGHT[d] * bump for d in range(3)])
    return pts


def at(m, i, j, k):
    return (k * (m + 1) + j) * (m + 1) + i


def hexes(m):
    return [
        [at(m, i + a, j + b, k + c) for a, b, c in
         ((0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0), (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1))]
        for k in range(m) for j in range(m) for i in range(m)
    ]


def tets(m, nodes):
    edges, elements = {}, []

    def mid(x, y):
        key = (min(x, y), max(x, y))
        if key not in edges:
            nodes.append([(nodes[x][d] + nodes[y][d]) / 2 for d in range(3)])
            edges[key] = len(nodes) - 1
        return edges[key]

    for k, j, i in itertools.product(range(m), repeat=3):
        for p in itertools.permutations(range(3)):
            c = [[i, j, k]]
            for s in range(3):
                nxt = list(c[-1])
                nxt[p[s]] += 1
                c.append(nxt)
            q = [at(m, *v) for v in c]
            r = [[nodes[q[n]][d] - nodes[q[0]][d] for d in range(3)] for n in (1, 2, 3)]
            det = (r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
                   - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
                   + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]))
            if det < 0:
                q[2], q[3] = q[3], q[2]
            q += [mid(q[0], q[1]), mid(q[1], q[2]), mid(q[2], q[0]),
                  mid(q[0], q[3]), mid(q[1], q[3]), mid(q[2], q[3])]
            elements.append(q)
    return elements


def write(path, ty, nodes, elements):
    out = ["*NODE"]
    out += [f"{i + 1}, {p[0]:.17e}, {p[1]:.17e}, {p[2]:.17e}" for i, p in enumerate(nodes)]
    out.append(f"*ELEMENT, TYPE={ty}")
    out += [f"{i + 1}, " + ", ".join(str(n + 1) for n in e) for i, e in enumerate(elements)]
    out.append(f"*ELSET, ELSET=ALL, GENERATE\n1, {len(elements)}, 1")
    out.append("*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.3\n*SOLID SECTION, ELSET=ALL, MATERIAL=M")
    ids = [str(i + 1) for i, p in enumerate(nodes)
           if any(abs(x) < 1e-12 or abs(x - 1) < 1e-12 for x in p)]
    out.append("*NSET, NSET=BOUNDARY")
    out += [", ".join(ids[n:n + 16]) for n in range(0, len(ids), 16)]
    path.write_text("\n".join(out) + "\n")


for name, (ty, kind, levels) in CASES.items():
    folder = ROOT / name
    folder.mkdir(parents=True, exist_ok=True)
    for m in levels:
        nodes = lattice(m)
        elements = hexes(m) if kind == "hex" else tets(m, nodes)
        write(folder / f"mesh-{m}.inp", ty, nodes, elements)
