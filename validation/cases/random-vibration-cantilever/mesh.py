"""Write mesh.inp with gmsh. Run: python3 mesh.py."""
GMSH_TYPE = 17
CCX_TYPE = "C3D20R"
ORDER = [0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 13, 9, 16, 18, 19, 17, 10, 12, 14, 15]
NODE_SETS = {"FIXED": lambda x,y,z: abs(x)<1e-7, "TIP": lambda x,y,z: abs(x-100)<1e-7}
OUTPUT = "mesh.inp"

import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix="fh-mapped-") as tmp:
    msh = Path(tmp) / "mesh.msh"
    subprocess.run(["gmsh", str(HERE / "mesh.geo"), "-3", "-format", "msh2",
                    "-o", str(msh), "-v", "2"], check=True)
    lines = msh.read_text().splitlines()
i = lines.index("$Nodes")
nodes = {int(a[0]): tuple(map(float, a[1:]))
         for line in lines[i + 2:i + 2 + int(lines[i + 1])]
         for a in [line.split()]}
i = lines.index("$Elements")
elements = []
for line in lines[i + 2:i + 2 + int(lines[i + 1])]:
    a = list(map(int, line.split()))
    if a[1] == GMSH_TYPE:
        connectivity = a[3 + a[2]:]
        elements.append([connectivity[k] for k in ORDER])
assert elements, "gmsh produced no case elements"
used = {n for e in elements for n in e}
assert used == set(nodes), "mesh has unused nodes or unexpected elements"
with (HERE / OUTPUT).open("w") as f:
    f.write("** Mapped mesh from mesh.geo and mesh.py. Units: SI-mm.\n*NODE, NSET=NALL\n")
    for n, p in nodes.items():
        f.write(f"{n}, " + ", ".join(f"{v:.12g}" for v in p) + "\n")
    f.write(f"*ELEMENT, TYPE={CCX_TYPE}, ELSET=ALL\n")
    for e, ns in enumerate(elements, 1):
        # CalculiX accepts at most 16 entries on the first element line.
        f.write(f"{e}, " + ", ".join(map(str, ns[:15])) + (",\n" if len(ns) > 15 else "\n"))
        if len(ns) > 15:
            f.write(", ".join(map(str, ns[15:])) + "\n")

    def write_set(kind, name, members):
        assert members, name
        f.write(f"*{kind}, {kind}={name}\n")
        for start in range(0, len(members), 16):
            f.write(", ".join(map(str, members[start:start + 16])) + "\n")

    for name, select in NODE_SETS.items():
        write_set("NSET", name, [n for n, p in nodes.items() if select(*p)])
print(f"{len(nodes)} nodes, {len(elements)} {CCX_TYPE} elements")
