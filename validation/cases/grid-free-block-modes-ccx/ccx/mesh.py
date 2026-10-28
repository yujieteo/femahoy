"""Write the CalculiX input of the reference solve of grid-free-block-modes-ccx.

The mesh is a structured grid of C3D20 elements with the cell size H (argument 1, in mm). Argument 2 is the output
file. The problem is the same as ../block/grid.toml: the free steel block, with no support, and a *FREQUENCY step of
12 modes.

    python3 mesh.py 1.25 job.inp
    ccx job
    python3 result.py job.dat
"""
import sys

H = float(sys.argv[1])
OUT = sys.argv[2]
L = (60.0, 40.0, 10.0)
N = [max(1, round(a / H)) for a in L]

nodes = {}


def node(i, j, k):
    """The node at the half-cell index (i, j, k)."""
    key = (i, j, k)
    if key not in nodes:
        nodes[key] = len(nodes) + 1
    return nodes[key]


elements = []
for i in range(N[0]):
    for j in range(N[1]):
        for k in range(N[2]):
            a, b, c = 2 * i, 2 * j, 2 * k
            corner = [(a, b, c), (a + 2, b, c), (a + 2, b + 2, c), (a, b + 2, c),
                      (a, b, c + 2), (a + 2, b, c + 2), (a + 2, b + 2, c + 2), (a, b + 2, c + 2)]
            mid = [(a + 1, b, c), (a + 2, b + 1, c), (a + 1, b + 2, c), (a, b + 1, c),
                   (a + 1, b, c + 2), (a + 2, b + 1, c + 2), (a + 1, b + 2, c + 2), (a, b + 1, c + 2),
                   (a, b, c + 1), (a + 2, b, c + 1), (a + 2, b + 2, c + 1), (a, b + 2, c + 1)]
            elements.append([node(*p) for p in corner + mid])

with open(OUT, "w") as f:
    f.write("*NODE\n")
    for (i, j, k), n in sorted(nodes.items(), key=lambda t: t[1]):
        x = [L[0] * i / (2 * N[0]), L[1] * j / (2 * N[1]), L[2] * k / (2 * N[2])]
        f.write(f"{n}, {x[0]:.12g}, {x[1]:.12g}, {x[2]:.12g}\n")
    f.write("*ELEMENT, TYPE=C3D20, ELSET=BOX\n")
    for e, conn in enumerate(elements, 1):
        f.write(f"{e}, " + ", ".join(map(str, conn[:15])) + ",\n" + ", ".join(map(str, conn[15:])) + "\n")
    f.write("*MATERIAL, NAME=STEEL\n*ELASTIC\n210000., 0.3\n*DENSITY\n7.85e-9\n")
    f.write("*SOLID SECTION, ELSET=BOX, MATERIAL=STEEL\n")
    f.write("*STEP\n*FREQUENCY\n12\n*END STEP\n")
print(f"{len(nodes)} nodes, {len(elements)} elements")
