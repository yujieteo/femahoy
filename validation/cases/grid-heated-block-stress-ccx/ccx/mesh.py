"""Write the CalculiX input of the reference solve of grid-heated-block-stress-ccx.

The mesh is a structured grid of C3D20 elements with the cell size H (argument 1, in m). Argument 2 is the output file.
The problem is the same as ../block/grid.toml: the plate and the heated block of grid-heated-block-ccx, with a coupled
steady temperature and displacement. As in the grid case, the model is the quarter x >= 0.05, y >= 0.05: the planes of
symmetry hold ux (x = 0.05) and uy (y = 0.05), and the centre of the bottom holds uz.

    python3 mesh.py 0.00125 job.inp
    ccx job
    python3 result.py job.dat
"""
import sys

H = float(sys.argv[1])
OUT = sys.argv[2]


def planes(a, b, n):
    return [a + (b - a) * i / n for i in range(n + 1)]


def axis(cuts, h):
    """Planes through each cut, with cells of about h."""
    p = [cuts[0]]
    for a, b in zip(cuts, cuts[1:]):
        n = max(1, round((b - a) / h))
        p += planes(a, b, n)[1:]
    return p


xs = axis([0.05, 0.065, 0.1], H)
ys = xs
zs = axis([0.0, 0.01, 0.03], H)


def solid(x, y, z):
    """The material of the cell with this centre: plate, block or none."""
    if z < 0.01:
        return "PLATE"
    if x < 0.065 and y < 0.065:
        return "BLOCK"
    return None


nodes = {}


def key(p):
    return tuple(round(v, 12) for v in p)


def node(p):
    k = key(p)
    if k not in nodes:
        nodes[k] = len(nodes) + 1
    return nodes[k]


elements = {"PLATE": [], "BLOCK": []}
bottom = []
for k in range(len(zs) - 1):
    for j in range(len(ys) - 1):
        for i in range(len(xs) - 1):
            x0, x1, y0, y1, z0, z1 = xs[i], xs[i + 1], ys[j], ys[j + 1], zs[k], zs[k + 1]
            part = solid((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2)
            if part is None:
                continue
            c = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
                 (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]
            mid = lambda a, b: tuple((p + q) / 2 for p, q in zip(c[a], c[b]))
            edges = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4),
                     (0, 4), (1, 5), (2, 6), (3, 7)]
            ids = [node(p) for p in c] + [node(mid(a, b)) for a, b in edges]
            elements[part].append(ids)
            if k == 0:
                bottom.append((part, len(elements[part])))

base = [n for p, n in nodes.items() if abs(p[2]) < 1e-9]
xsym = [n for p, n in nodes.items() if abs(p[0] - 0.05) < 1e-9]
ysym = [n for p, n in nodes.items() if abs(p[1] - 0.05) < 1e-9]
first = {"PLATE": 1, "BLOCK": len(elements["PLATE"]) + 1}
with open(OUT, "w") as f:
    f.write("** units: SI-m\n*NODE, NSET=NALL\n")
    for p, n in nodes.items():
        f.write(f"{n}, {p[0]:.12g}, {p[1]:.12g}, {p[2]:.12g}\n")
    for part in ("PLATE", "BLOCK"):
        f.write(f"*ELEMENT, TYPE=C3D20, ELSET={part}\n")
        for e, ids in enumerate(elements[part]):
            line = [first[part] + e] + ids
            f.write(", ".join(map(str, line[:16])) + ",\n")
            f.write(", ".join(map(str, line[16:])) + "\n")
    probes = {"BLOCK_TOP": (0.05, 0.05, 0.03), "PLATE_CORNER": (0.1, 0.1, 0.0),
              "PLATE_EDGE": (0.1, 0.05, 0.0), "CENTRE": (0.05, 0.05, 0.0)}
    for name, p in probes.items():
        f.write(f"*NSET, NSET={name}\n{nodes[key(p)]}\n")
    for name, ids in (("BASE", base), ("XSYM", xsym), ("YSYM", ysym)):
        f.write(f"*NSET, NSET={name}\n")
        for i in range(0, len(ids), 16):
            f.write(", ".join(map(str, ids[i:i + 16])) + "\n")
    f.write("*MATERIAL, NAME=STEEL\n*ELASTIC\n200.E9, 0.3\n*EXPANSION, ZERO=20.\n12.E-6\n"
            "*CONDUCTIVITY\n50.\n")
    f.write("*MATERIAL, NAME=COPPER\n*ELASTIC\n110.E9, 0.34\n*EXPANSION, ZERO=20.\n17.E-6\n"
            "*CONDUCTIVITY\n400.\n")
    f.write("*SOLID SECTION, ELSET=PLATE, MATERIAL=STEEL\n")
    f.write("*SOLID SECTION, ELSET=BLOCK, MATERIAL=COPPER\n")
    f.write("*INITIAL CONDITIONS, TYPE=TEMPERATURE\nNALL, 20.\n")
    f.write("*STEP\n*COUPLED TEMPERATURE-DISPLACEMENT, STEADY STATE\n")
    f.write("*BOUNDARY\nXSYM, 1, 1, 0.\nYSYM, 2, 2, 0.\nCENTRE, 3, 3, 0.\n")
    f.write("*DFLUX\nBLOCK, BF, 2.E6\n*FILM\n")
    for part, e in bottom:
        f.write(f"{first[part] + e - 1}, F1, 20., 200.\n")
    for name in ("BLOCK_TOP", "PLATE_CORNER", "PLATE_EDGE", "BASE"):
        f.write(f"*NODE PRINT, NSET={name}\nU, NT\n")
    f.write("*NODE PRINT, NSET=NALL, TOTALS=ONLY\nRF\n")
    f.write("*END STEP\n")
print(f"{len(nodes)} nodes, {sum(len(v) for v in elements.values())} elements")
