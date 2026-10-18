#!/usr/bin/env python3
"""Write the mesh of the case validation/cases/bimetal-strip/ (a quarter of a bimetal strip).

The strip is 80 x 4 x 2 mm. The quarter model is x in [0, 40], y in [0, 2], z in [0, 2],
with C3D8I elements of 1 x 1 x 0.5 mm. Layer 1 is z in [0, 1], layer 2 is z in [1, 2].
Run: python3 scripts/gen-bimetal-strip.py
"""
import pathlib

NX, NY, NZ = 40, 2, 4
DX, DY, DZ = 1.0, 1.0, 0.5
OUT = pathlib.Path(__file__).resolve().parent.parent / "validation" / "cases" / "bimetal-strip" / "strip.inp"


def node(i, j, k):
    return 1 + i + (NX + 1) * (j + (NY + 1) * k)


def ids(lines, values):
    for n in range(0, len(values), 16):
        lines.append(", ".join(str(v) for v in values[n : n + 16]))


def main():
    lines = ["** units: SI-mm", "** Quarter of a bimetal strip, made by scripts/gen-bimetal-strip.py", "*NODE"]
    grid = [(i, j, k) for k in range(NZ + 1) for j in range(NY + 1) for i in range(NX + 1)]
    for i, j, k in grid:
        lines.append(f"{node(i, j, k)}, {i * DX:g}, {j * DY:g}, {k * DZ:g}")
    lines.append("*ELEMENT, TYPE=C3D8I")
    layers = ([], [])
    e = 0
    for k in range(NZ):
        for j in range(NY):
            for i in range(NX):
                e += 1
                c = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
                n = [node(a, b, k) for a, b in c] + [node(a, b, k + 1) for a, b in c]
                lines.append(f"{e}, " + ", ".join(map(str, n)))
                layers[k >= NZ // 2].append(e)
    for name, members in (("LAYER1", layers[0]), ("LAYER2", layers[1])):
        lines.append(f"*ELSET, ELSET={name}")
        ids(lines, members)
    sets = {
        "ALL": [node(*p) for p in grid],
        "SYM_X": [node(*p) for p in grid if p[0] == 0],
        "SYM_Y": [node(*p) for p in grid if p[1] == 0],
        "ORIGIN": [node(0, 0, 0)],
        "P20": [node(20, 0, 0)],
    }
    for name, members in sets.items():
        lines.append(f"*NSET, NSET={name}")
        ids(lines, members)
    lines += [
        "*MATERIAL, NAME=LOW",
        "*ELASTIC",
        "200000, 0",
        "*EXPANSION",
        "1.2e-5",
        "*MATERIAL, NAME=HIGH",
        "*ELASTIC",
        "70000, 0",
        "*EXPANSION",
        "2.3e-5",
        "*SOLID SECTION, ELSET=LAYER1, MATERIAL=LOW",
        "*SOLID SECTION, ELSET=LAYER2, MATERIAL=HIGH",
        "*INITIAL CONDITIONS, TYPE=TEMPERATURE",
        "ALL, 20",
        "*STEP, NAME=HEAT",
        "*STATIC",
        "*BOUNDARY",
        "SYM_X, 1, 1",
        "SYM_Y, 2, 2",
        "ORIGIN, 3, 3",
        "*TEMPERATURE",
        "ALL, 120",
        "*END STEP",
    ]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
