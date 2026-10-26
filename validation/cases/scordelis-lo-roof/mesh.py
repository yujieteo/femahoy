"""Write part.inp with gmsh. Run: python3 mesh.py [N] (default 24)."""
GMSH_TYPE = 16
CCX_TYPE = "S8R"
ORDER = list(range(8))
OUTPUT = "part.inp"
NODE_SETS = {"END0": lambda x,y,z: abs(x)<1e-7,
             "END1": lambda x,y,z: abs(x-50000)<1e-7,
             "TIP": lambda x,y,z: abs(x-25000)<1e-7 and y>16069}

import sys
import subprocess
import tempfile
from pathlib import Path

N = int(sys.argv[1]) if len(sys.argv) > 1 else 24
if N < 2 or N % 2:
    raise ValueError("N must be a positive even count of 2 or more")
HERE = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix="fh-mapped-") as tmp:
    msh = Path(tmp) / "mesh.msh"
    subprocess.run(["gmsh", str(HERE / "mesh.geo"), "-3", "-format", "msh2",
                    "-o", str(msh), "-setnumber", "N", str(N), "-v", "2"], check=True)
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
nodes = {n: p for n, p in nodes.items() if n in used}
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
