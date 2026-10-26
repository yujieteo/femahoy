"""Write the package tensor grid. Run: python3 mesh.py [h] (default 6 mm).

Each material boundary cuts the whole grid. The UF2 fillet has 3 steps,
each 0.3 mm wide, as in the mesh audit. Adjacent blocks share nodes.
"""

import math
import sys
from pathlib import Path

H = float(sys.argv[1]) if len(sys.argv) > 1 else 6.0
if H <= 0:
    raise ValueError("h must be positive")

# Material boxes: x0, y0, z0, x1, y1, z1.
BOXES = {
    "SUB": (0, 0, 0, 27.5, 27.5, 1.81),
    "UF2": (0, 0, 1.81, 20.5, 13.75, 1.91),
    "INT": (0, 0, 1.91, 20.5, 13.75, 2.0),
    "UF1": (0, 0, 2.0, 19.5, 12.2, 2.04),
    "DIE": (0, 0, 2.04, 19.5, 12.2, 2.73),
    "EMC_X": (19.5, 0, 2.0, 20.5, 13.75, 2.73),
    "EMC_Y": (0, 12.2, 2.0, 19.5, 13.75, 2.73),
    "ADH_X": (24.3, 0, 1.81, 27.5, 27.5, 1.87),
    "ADH_Y": (0, 20.1, 1.81, 24.3, 27.5, 1.87),
    "FRM_X": (24.3, 0, 1.87, 27.5, 27.5, 2.745),
    "FRM_Y": (0, 20.1, 1.87, 24.3, 27.5, 2.745),
}
# The middle height of each third sets the step height.
for j in range(3):
    top = 1.81 + 0.92 * (1 - (j + 0.5) / 3)
    BOXES[f"FIL_X_{j}"] = (20.5 + j * .3, 0, 1.81, 20.5 + (j + 1) * .3, 13.75, top)
    BOXES[f"FIL_Y_{j}"] = (0, 13.75 + j * .3, 1.81, 20.5, 13.75 + (j + 1) * .3, top)
    for k in range(3):
        corner_top = 1.81 + 0.92 * (1 - (max(j, k) + 0.5) / 3)
        BOXES[f"FIL_C_{j}_{k}"] = (20.5 + j * .3, 13.75 + k * .3, 1.81,
                                        20.5 + (j + 1) * .3, 13.75 + (k + 1) * .3, corner_top)


def axis(k):
    cuts = sorted({round(b[i], 10) for b in BOXES.values() for i in (k, k + 3)})
    values = []
    for a, b in zip(cuts, cuts[1:]):
        n = max(1, math.ceil((b - a) / H - 1e-9))
        if k == 2 and a == 0:
            n = 4  # The audit uses 4 elements through the substrate.
        values.extend(a + (b - a) * i / n for i in range(n))
    return values + cuts[-1:]


X, Y, Z = (axis(k) for k in range(3))
points, ids, elements, sets = [], {}, [], {}


def node(p):
    key = tuple(round(v, 10) for v in p)
    if key not in ids:
        ids[key] = len(points) + 1
        points.append(p)
    return ids[key]


EDGES = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4),
         (0, 4), (1, 5), (2, 6), (3, 7)]
for x0, x1 in zip(X, X[1:]):
    for y0, y1 in zip(Y, Y[1:]):
        for z0, z1 in zip(Z, Z[1:]):
            centre = ((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2)
            names = [name for name, b in BOXES.items()
                     if all(b[k] < centre[k] < b[k + 3] for k in range(3))]
            if not names:
                continue
            assert len(names) == 1, names
            name = names[0]
            if name.startswith("FIL_"):
                name = "_".join(name.split("_")[:2])
            p = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
                 (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]
            ns = [node(q) for q in p]
            ns += [node(tuple((p[a][k] + p[b][k]) / 2 for k in range(3))) for a, b in EDGES]
            elements.append(ns)
            sets.setdefault(name, []).append(len(elements))

with Path(__file__).with_name("mesh.inp").open("w") as f:
    f.write(f"** Mapped C3D20R tensor grid, h={H:g} mm. Units: SI-mm.\n*NODE, NSET=NALL\n")
    for i, p in enumerate(points, 1):
        f.write(f"{i}, " + ", ".join(f"{v:.12g}" for v in p) + "\n")
    f.write("*ELEMENT, TYPE=C3D20R, ELSET=ALL\n")
    for i, ns in enumerate(elements, 1):
        f.write(f"{i}, " + ", ".join(map(str, ns[:15])) + ",\n")
        f.write(", ".join(map(str, ns[15:])) + "\n")

    def write_set(kind, name, members):
        assert members, name
        f.write(f"*{kind}, {kind}={name}\n")
        for start in range(0, len(members), 16):
            f.write(", ".join(map(str, members[start:start + 16])) + "\n")

    for name, members in sets.items():
        write_set("ELSET", name, members)
    for name, select in {
        "SYM_X": lambda x, y, z: abs(x) < 1e-8,
        "SYM_Y": lambda x, y, z: abs(y) < 1e-8,
        "BOTTOM": lambda x, y, z: abs(z) < 1e-8,
        "ORIGIN": lambda x, y, z: abs(x) + abs(y) + abs(z) < 1e-8,
        "CORNER": lambda x, y, z: abs(x - 27.5) + abs(y - 27.5) + abs(z) < 1e-8,
    }.items():
        write_set("NSET", name, [i for i, p in enumerate(points, 1) if select(*p)])
print(f"{len(points)} nodes, {len(elements)} C3D20R elements")
