"""Write the CalculiX input of the reference solve of grid-radiating-fin-ccx.

The mesh is the grid of ../fin/grid.toml: a uniform grid of C3D8 (DC3D8) elements with the cell
size H (argument 1, in m). Argument 2 is the output file. The problem is the same as
../fin/grid.toml: a square pin 0.004 x 0.004 x 0.05 m with the conductivity table of the steel,
600 K at the base z = 0, and a radiation (emissivity 0.9) to surroundings at 300 K on all other
faces.

    python3 mesh.py 0.001 job.inp
"""
import sys

H = float(sys.argv[1])
OUT = sys.argv[2]
W, L = 0.004, 0.05
nx, nz = round(W / H), round(L / H)
xs = [W * i / nx for i in range(nx + 1)]
zs = [L * k / nz for k in range(nz + 1)]


def node(i, j, k):
    return 1 + i + (nx + 1) * (j + (nx + 1) * k)


elements = []
# The radiating faces: (element, face label). C3D8 faces: S1 z-, S2 z+, S3 y-, S4 x+, S5 y+, S6 x-.
faces = []
for k in range(nz):
    for j in range(nx):
        for i in range(nx):
            ids = [node(i, j, k), node(i + 1, j, k), node(i + 1, j + 1, k), node(i, j + 1, k),
                   node(i, j, k + 1), node(i + 1, j, k + 1), node(i + 1, j + 1, k + 1),
                   node(i, j + 1, k + 1)]
            elements.append(ids)
            e = len(elements)
            if k == nz - 1:
                faces.append((e, 2))
            if j == 0:
                faces.append((e, 3))
            if i == nx - 1:
                faces.append((e, 4))
            if j == nx - 1:
                faces.append((e, 5))
            if i == 0:
                faces.append((e, 6))

with open(OUT, "w") as f:
    f.write("** units: SI-m\n*NODE, NSET=NALL\n")
    for k, z in enumerate(zs):
        for j, y in enumerate(xs):
            for i, x in enumerate(xs):
                f.write(f"{node(i, j, k)}, {x:.12g}, {y:.12g}, {z:.12g}\n")
    f.write("*ELEMENT, TYPE=DC3D8, ELSET=FIN\n")
    for e, ids in enumerate(elements, 1):
        f.write(f"{e}, " + ", ".join(map(str, ids)) + "\n")
    f.write("*NSET, NSET=BASE\n")
    for j in range(nx + 1):
        for i in range(nx + 1):
            f.write(f"{node(i, j, 0)},\n")
    f.write("*NSET, NSET=TIP\n%d,\n" % node(nx // 2, nx // 2, nz))
    f.write("*NSET, NSET=MIDDLE\n%d,\n" % node(nx // 2, nx // 2, nz // 2))
    f.write("*MATERIAL, NAME=STEEL\n*CONDUCTIVITY\n14.0, 300.0\n17.5, 500.0\n20.0, 700.0\n")
    f.write("*SOLID SECTION, ELSET=FIN, MATERIAL=STEEL\n")
    f.write("*PHYSICAL CONSTANTS, ABSOLUTE ZERO=0.0, STEFAN BOLTZMANN=5.670374419E-8\n")
    f.write("*INITIAL CONDITIONS, TYPE=TEMPERATURE\nNALL, 300.0\n")
    f.write("*STEP, INC=100\n*HEAT TRANSFER, STEADY STATE\n1.0, 1.0\n")
    f.write("*BOUNDARY\nBASE, 11, 11, 600.0\n")
    f.write("*RADIATE\n")
    for e, s in faces:
        f.write(f"{e}, R{s}, 300.0, 0.9\n")
    f.write("*NODE PRINT, NSET=TIP\nNT\n*NODE PRINT, NSET=MIDDLE\nNT\n")
    f.write("*NODE PRINT, NSET=BASE, TOTALS=ONLY\nRFL\n")
    f.write("*END STEP\n")
