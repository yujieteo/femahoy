"""Write the CalculiX input of the reference solve of grid-plate-hole-stress-ccx from the gmsh mesh.

    gmsh ../../grid-plate-hole-ccx/ccx/plate.geo -3 -setnumber H 0.002 -o mesh.inp
    python3 job.py mesh.inp job.inp
    ccx job

The problem is the same as ../plate/grid.toml: the plate and the pad of grid-plate-hole-ccx,
heated from 20 C to 120 C. The wall of the hole holds the 3 displacement components. The steel has E = 200 GPa, nu = 0.3 and
alpha = 12e-6 /K, the copper E = 110 GPa, nu = 0.34 and alpha = 17e-6 /K, both with no strain
at 20 C. The elements are C3D10. The nodes of the hole wall are the nodes at the radius of the
hole.
"""
import math
import sys

MESH, OUT = sys.argv[1], sys.argv[2]

nodes = {}
elements = []  # (gmsh elset, line)
section = None
elset = None
with open(MESH) as f:
    for line in f:
        s = line.strip()
        if not s or s.startswith("**"):
            continue
        if s.startswith("*"):
            key = s.upper()
            section = None
            if key.startswith("*NODE"):
                section = "node"
            elif key.startswith("*ELEMENT"):
                section = "element"
                kind = [p.split("=")[1] for p in key.replace(" ", "").split(",") if p.startswith("TYPE=")][0]
                if kind != "C3D10":
                    section = None
                    continue
                elset = [p.split("=")[1] for p in key.replace(" ", "").split(",") if p.startswith("ELSET=")][0]
            continue
        if section == "node":
            v = [x for x in s.split(",") if x.strip()]
            nodes[int(v[0])] = tuple(float(x) for x in v[1:4])
        elif section == "element":
            elements.append((elset, s))

used = set()
for _, line in elements:
    for v in line.rstrip(",").split(",")[1:]:
        used.add(int(v))


def near(p, tol=1e-9):
    hits = [n for n in used if all(abs(a - b) < tol for a, b in zip(nodes[n], p))]
    assert len(hits) == 1, (p, hits)
    return hits[0]


hole = [n for n in used if abs(math.hypot(nodes[n][0] - 0.03, nodes[n][1] - 0.03) - 0.01) < 1e-7]
probes = {
    "PAD_CORNER": (0.09, 0.04, 0.02),
    "PLATE_FAR": (0.1, 0.0, 0.0),
    "PLATE_NEAR": (0.0, 0.06, 0.01),
}
# The part of each element from its centroid: the pad is above z = 0.01.
sets = {"PLATE": [], "PAD": []}
for _, line in elements:
    corners = [nodes[int(v)] for v in line.rstrip(",").split(",")[1:5]]
    z = sum(c[2] for c in corners) / 4
    sets["PAD" if z > 0.01 else "PLATE"].append(line)
with open(OUT, "w") as f:
    f.write("** units: SI-m\n*NODE, NSET=NALL\n")
    for n in sorted(used):
        p = nodes[n]
        f.write(f"{n}, {p[0]:.15g}, {p[1]:.15g}, {p[2]:.15g}\n")
    for name in ("PLATE", "PAD"):
        f.write(f"*ELEMENT, TYPE=C3D10, ELSET={name}\n")
        for line in sets[name]:
            f.write(line.rstrip(",") + "\n")
    f.write("*NSET, NSET=HOLE\n")
    for k in range(0, len(hole), 8):
        f.write(", ".join(map(str, sorted(hole)[k:k + 8])) + "\n")
    for name, p in probes.items():
        f.write(f"*NSET, NSET={name}\n{near(p)}\n")
    f.write("*MATERIAL, NAME=STEEL\n*ELASTIC\n200.E9, 0.3\n*EXPANSION, ZERO=20.\n12.E-6\n")
    f.write("*MATERIAL, NAME=COPPER\n*ELASTIC\n110.E9, 0.34\n*EXPANSION, ZERO=20.\n17.E-6\n")
    f.write("*SOLID SECTION, ELSET=PLATE, MATERIAL=STEEL\n")
    f.write("*SOLID SECTION, ELSET=PAD, MATERIAL=COPPER\n")
    f.write("*INITIAL CONDITIONS, TYPE=TEMPERATURE\nNALL, 20.\n")
    f.write("*STEP\n*STATIC\n*BOUNDARY\nHOLE, 1, 3, 0.\n*TEMPERATURE\nNALL, 120.\n")
    for name in probes:
        f.write(f"*NODE PRINT, NSET={name}\nU\n")
    f.write("*END STEP\n")
print(f"{len(used)} nodes, {len(elements)} elements, {len(hole)} hole nodes")
