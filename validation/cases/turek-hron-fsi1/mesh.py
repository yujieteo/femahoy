"""Write the meshes of the Turek and Hron FSI1 case: the fluid mesh (fluid/system/blockMeshDict)
and the flag mesh (solid/mesh.inp).

Usage, in the repository root: python3 validation/cases/turek-hron-fsi1/mesh.py [LEVEL [OUT]]

The defaults (level 1, validation/cases/turek-hron-fsi1/fsi1) write the meshes of the check.
Level 1 is the coarsest mesh. Each level doubles the cells in each direction. derivation.md gives
the mesh study.

The fluid mesh is 2D, 19 blocks and 1 cell of 1 m in z. An O-grid of 5 blocks goes around the
cylinder to the square x, y = 0.1 to 0.3. The flag cuts it on the right. The other blocks fill the
channel. The flag mesh is C3D8I bricks of 1 m in z, in rows along the flag. Its clamped end is the
arc of the cylinder. The 2 meshes have z = 0 to 1, as the quasi-2D coupling of the CalculiX
adapter needs.
"""
import math
import os
import sys

L, H = 2.5, 0.41               # channel
C, R = (0.2, 0.2), 0.05        # cylinder
X_TIP, H_FLAG = 0.6, 0.02      # flag: from the cylinder to x = 0.6, 0.02 thick about y = 0.2
BOX = (0.1, 0.3)               # x and y of the square of the O-grid
Y0, Y1 = C[1] - H_FLAG / 2, C[1] + H_FLAG / 2
X_ROOT = C[0] + math.sqrt(R * R - (H_FLAG / 2) ** 2)
TH0 = math.degrees(math.asin(H_FLAG / 2 / R))


def cyl(deg):
    a = math.radians(deg)
    return (C[0] + R * math.cos(a), C[1] + R * math.sin(a))


def ratio(first, length, n):
    """The ratio of the last to the first cell of n cells over length, with the first cell size."""
    lo, hi = 1e-6, 1e6
    for _ in range(200):
        mid = math.sqrt(lo * hi)
        r = mid ** (1 / (n - 1)) if n > 1 else 1
        total = first * (n if abs(r - 1) < 1e-12 else (r ** n - 1) / (r - 1))
        lo, hi = (mid, hi) if total < length else (lo, mid)
    return math.sqrt(lo * hi)


def block_mesh_dict(level):
    """The text of system/blockMeshDict, and the number of cells."""
    k = 2 ** (level - 1)
    nr, nq, ns, nh = 12 * k, 20 * k, 8 * k, 4 * k   # radial, quarter arc, side, flag thickness
    nf, nu, nb, nt, nw = 24 * k, 8 * k, 8 * k, 9 * k, 40 * k  # along flag, inlet, bands, wake
    gr = 4.0                                         # outer to inner radial cell size
    gw = ratio((X_TIP - BOX[1]) / nf, L - X_TIP, nw)  # wake: first cell as along the flag
    xs, ys = (0.0, BOX[0], BOX[1], X_TIP, L), (0.0, BOX[0], Y0, Y1, BOX[1], H)
    pts = {}

    def v(name, p):
        pts.setdefault(name, (len(pts), p))
        return name

    for i, x in enumerate(xs):
        for j, y in enumerate(ys):
            v(f"g{i}{j}", (x, y))
    for deg in (45, 135, 225, 315):
        v(f"c{deg}", cyl(deg))
    v("ct", (X_ROOT, Y1))
    v("cb", (X_ROOT, Y0))
    blocks, arcs = [], []

    def block(corners, n1, n2, grading="1 1"):
        blocks.append((corners, n1, n2, grading))

    # O-grid: inner start, outer start, outer end, inner end. Radial x1, anticlockwise x2.
    ring = [("c225", "g11", "g21", "c315", 225, 315, nq),
            ("c315", "g21", "g22", "cb", 315, 360 - TH0, ns),
            ("ct", "g23", "g24", "c45", TH0, 45, ns),
            ("c45", "g24", "g14", "c135", 45, 135, nq),
            ("c135", "g14", "g11", "c225", 135, 225, nq)]
    for a, b, c, d, t0, t1, n in ring:
        block((a, b, c, d), nr, n, f"{gr} 1")
        arcs.append((d, a, cyl((t0 + t1) / 2)))
    # Rectangles: x indices, y indices, cells in x and y, grading in x.
    rects = [(0, 1, 1, 4, nu, nq, 1), (2, 3, 1, 2, nf, ns, 1), (2, 3, 3, 4, nf, ns, 1),
             (3, 4, 1, 2, nw, ns, gw), (3, 4, 2, 3, nw, nh, gw), (3, 4, 3, 4, nw, ns, gw)]
    for i, n in enumerate((nu, nq, nf, nw)):
        g = gw if i == 3 else 1
        rects += [(i, i + 1, 0, 1, n, nb, g), (i, i + 1, 4, 5, n, nt, g)]
    for i0, i1, j0, j1, n1, n2, g in rects:
        block((f"g{i0}{j0}", f"g{i1}{j0}", f"g{i1}{j1}", f"g{i0}{j1}"), n1, n2, f"{g:.6g} 1")
    used = {c for b in blocks for c in b[0]}
    names = [n for n, _ in sorted(pts.items(), key=lambda kv: kv[1][0]) if n in used]
    index = {n: i for i, n in enumerate(names)}
    out = ["FoamFile { version 2.0; format ascii; class dictionary; object blockMeshDict; }\n",
           "// Written by validation/cases/turek-hron-fsi1/mesh.py, level %d.\n" % level,
           "scale 1;\n\nvertices\n("]
    for z in (0, 1):
        out += ["    (%.9g %.9g %d)" % (*pts[n][1], z) for n in names]
    out.append(");\n\nblocks\n(")
    m = len(names)
    for corners, n1, n2, g in blocks:
        ids = [index[c] for c in corners]
        hexes = " ".join(str(i) for i in ids + [i + m for i in ids])
        out.append(f"    hex ({hexes}) ({n1} {n2} 1) simpleGrading ({g} 1)")
    out.append(");\n\nedges\n(")
    for a, b, mid in arcs:
        for z in (0, 1):
            out.append("    arc %d %d (%.9g %.9g %d)" % (index[a] + z * m, index[b] + z * m,
                                                       *mid, z))
    out.append(");\n")

    def face(a, b):
        i, j = index[a], index[b]
        return f"({i} {j} {j + m} {i + m})"

    patches = {
        "inlet": [face("g01", "g00"), face("g04", "g01"), face("g05", "g04")],
        "outlet": [face(f"g4{j}", f"g4{j+1}") for j in (0, 1, 2, 3, 4)],
        "walls": [face(f"g{i}0", f"g{i+1}0") for i in range(4)]
        + [face(f"g{i+1}5", f"g{i}5") for i in range(4)],
        "cylinder": [face(a, d) for a, _, _, d, *_ in ring],
        "flag": [face("cb", "g22"), face("g22", "g32"), face("g32", "g33"),
                 face("g33", "g23"), face("g23", "ct")],
    }
    out.append("boundary\n(")
    for name, faces in patches.items():
        out.append(f"    {name}\n    {{\n        type wall;\n        faces\n        (")
        out += [f"            {f}" for f in faces]
        out.append("        );\n    }")
    out.append("    frontAndBack\n    {\n        type empty;\n        faces\n        (")
    for corners, *_ in blocks:
        ids = [index[c] for c in corners]
        out.append("            (%d %d %d %d)" % (ids[0], ids[3], ids[2], ids[1]))
        out.append("            (%d %d %d %d)" % tuple(i + m for i in ids))
    out.append("        );\n    }\n);\n")
    cells = sum(n1 * n2 for _, n1, n2, _ in blocks)
    return "\n".join(out), cells


def flag_mesh(level):
    """The text of solid/mesh.inp: nodes, C3D8I elements and the sets, and the element count."""
    k = 2 ** (level - 1)
    nx, ny = 24 * k, 4 * k
    lines = ["** units: SI-m", "** Written by validation/cases/turek-hron-fsi1/mesh.py, "
             f"level {level}.", "*NODE, NSET=NALL"]
    ids = {}
    for z in (0, 1):
        for j in range(ny + 1):
            y = Y0 + H_FLAG * j / ny
            x0 = C[0] + math.sqrt(R * R - (y - C[1]) ** 2)
            for i in range(nx + 1):
                n = len(ids) + 1
                ids[(i, j, z)] = n
                lines.append("%d, %.9g, %.9g, %d" % (n, x0 + (X_TIP - x0) * i / nx, y, z))
    lines.append("*ELEMENT, TYPE=C3D8I, ELSET=EALL")
    e = 0
    for j in range(ny):
        for i in range(nx):
            e += 1
            q = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
            nodes = [ids[(a, b, 0)] for a, b in q] + [ids[(a, b, 1)] for a, b in q]
            lines.append("%d, %s" % (e, ", ".join(map(str, nodes))))

    def nset(name, keys):
        lines.append(f"*NSET, NSET={name}")
        nodes = sorted(ids[key] for key in keys)
        lines.extend(", ".join(map(str, nodes[i:i + 8])) for i in range(0, len(nodes), 8))

    edge = [(i, j, z) for i in range(nx + 1) for j in range(ny + 1) for z in (0, 1)]
    nset("NROOT", [n for n in edge if n[0] == 0])
    nset("NINTERFACE", [n for n in edge if n[1] in (0, ny) or n[0] == nx])
    nset("NTIP", [(nx, ny // 2, z) for z in (0, 1)])
    return "\n".join(lines) + "\n", e


def main():
    level = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    here = os.path.dirname(os.path.abspath(__file__))
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "fsi1")
    text, cells = block_mesh_dict(level)
    os.makedirs(os.path.join(out, "fluid/system"), exist_ok=True)
    with open(os.path.join(out, "fluid/system/blockMeshDict"), "w") as f:
        f.write(text)
    text, elements = flag_mesh(level)
    os.makedirs(os.path.join(out, "solid"), exist_ok=True)
    with open(os.path.join(out, "solid/mesh.inp"), "w") as f:
        f.write(text)
    print(f"level {level}: {cells} fluid cells, {elements} flag elements in {out}")


if __name__ == "__main__":
    main()
