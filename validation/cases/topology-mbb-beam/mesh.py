"""Write 60x20/job.inp: the half MBB beam of 60 x 20 square elements of side 1.

Node (i, j) is at x = i, y = j. The left edge is the symmetry plane (ux = 0), the bottom right
corner is the roller support (uy = 0), and the load of 1 acts down at the top left corner.
Run: python3 mesh.py
"""

from pathlib import Path

NX, NY = 60, 20


def node(i, j):
    return i * (NY + 1) + j + 1


lines = [
    "** The half MBB beam of Andreassen et al. (2011): 60 x 20 square elements of side 1.",
    "** The benchmark has no units. Written by mesh.py.",
    "*NODE",
]
lines += [f"{node(i, j)}, {i}, {j}" for i in range(NX + 1) for j in range(NY + 1)]
lines.append("*ELEMENT, TYPE=CPE4, ELSET=BEAM")
for i in range(NX):
    for j in range(NY):
        corners = [node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1)]
        lines.append(f"{i * NY + j + 1}, " + ", ".join(map(str, corners)))
lines += ["*NSET, NSET=SYMMETRY"]
lines += [f"{node(0, j)}," for j in range(NY + 1)]
lines += [
    "*NSET, NSET=SUPPORT",
    f"{node(NX, 0)},",
    "*NSET, NSET=LOAD",
    f"{node(0, NY)},",
    "*BOUNDARY",
    "SYMMETRY, 1, 1",
    "SUPPORT, 2, 2",
    "*STEP",
    "*STATIC",
    "*CLOAD",
    "LOAD, 2, -1.0",
    "*NODE PRINT, NSET=LOAD",
    "U",
    "*NODE FILE",
    "U",
    "*END STEP",
]
out = Path(__file__).parent / "60x20" / "job.inp"
out.parent.mkdir(exist_ok=True)
out.write_text("\n".join(lines) + "\n")
