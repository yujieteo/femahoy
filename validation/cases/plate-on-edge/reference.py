"""Write a ccx input of the plate on a graded hex mesh (C3D8I), for the reference values.

Usage: python3 reference.py LEVEL [NZ] > plate.inp
Level 0 has 1 mm elements at each pad edge and plate corner, growing by 1.3 to 40 mm.
Each level halves every element in x and y. NZ is the number of layers through the thickness.
"""
import math, sys

level = int(sys.argv[1])
nz = int(sys.argv[2]) if len(sys.argv) > 2 else 1
L2, H2, T = 1878.0, 500.0, 2.4
HMIN, GROW, HMAX = 1.0, 1.3, 40.0

def axis(keys, even=()):
    """Node positions along one axis: graded to each key point, then split for the level."""
    out = [keys[0]]
    for a, b in zip(keys, keys[1:]):
        h = lambda s: min(HMAX, HMIN + (GROW - 1.0) * min(s - a, b - s))
        m = 4000
        cum = [0.0]
        for i in range(m):
            s0, s1 = a + (b - a) * i / m, a + (b - a) * (i + 1) / m
            cum.append(cum[-1] + (s1 - s0) * 0.5 * (1 / h(s0) + 1 / h(s1)))
        n = max(1, round(cum[-1]))
        if (a, b) in even and n % 2:
            n += 1
        j = 0
        for i in range(1, n):
            t = cum[-1] * i / n
            while cum[j + 1] < t:
                j += 1
            f = (t - cum[j]) / (cum[j + 1] - cum[j])
            out.append(a + (b - a) * (j + f) / m)
        out.append(b)
    k = 2 ** level
    return [p + (q - p) * i / k for p, q in zip(out, out[1:]) for i in range(k)] + [out[-1]]

xk = sorted({-L2, L2, *(s * v for s in (-1, 1) for v in (1550, 1500, 1450, 1310, 1290, 1250, 1200, 1150))})
xs = axis(xk, even={(-1150.0, 1150.0)})
ys = axis([-H2, H2])
zs = [-T / 2 + T * i / nz for i in range(nz + 1)]
nx, ny = len(xs) - 1, len(ys) - 1
nid = lambda i, j, k: 1 + i + (nx + 1) * (j + (ny + 1) * k)
w = sys.stdout.write
w("*NODE\n")
for k, z in enumerate(zs):
    for j, y in enumerate(ys):
        for i, x in enumerate(xs):
            w(f"{nid(i, j, k)}, {x:.10g}, {y:.10g}, {z:.10g}\n")
w("*ELEMENT, TYPE=C3D8I, ELSET=EALL\n")
eid = lambda i, j, k: 1 + i + nx * (j + ny * k)
for k in range(nz):
    for j in range(ny):
        for i in range(nx):
            a, b = (i, j, k), (i, j, k + 1)
            c = [nid(i, j, k), nid(i + 1, j, k), nid(i + 1, j + 1, k), nid(i, j + 1, k),
                 nid(i, j, k + 1), nid(i + 1, j, k + 1), nid(i + 1, j + 1, k + 1), nid(i, j + 1, k + 1)]
            w(f"{eid(i, j, k)}, " + ", ".join(map(str, c)) + "\n")

def nset(name, ids):
    w(f"*NSET, NSET={name}\n")
    for p in range(0, len(ids), 10):
        w(", ".join(map(str, ids[p:p + 10])) + "\n")

col = lambda x0, x1, j: [nid(i, j, k) for k in range(nz + 1) for i in range(nx + 1) if x0 - 1e-6 <= xs[i] <= x1 + 1e-6]
nset("END_L", [nid(0, j, k) for k in range(nz + 1) for j in range(ny + 1)])
nset("END_R", [nid(nx, j, k) for k in range(nz + 1) for j in range(ny + 1)])
nset("PIN_L", col(-1310, -1290, 0))
nset("PIN_R", col(1290, 1310, 0))
for name, x in (("TOP_0", 0.0), ("TOP_P1500", 1500.0), ("TOP_M1500", -1500.0)):
    nset(name, col(x, x, ny))
w("*MATERIAL, NAME=STEEL\n*ELASTIC\n210000, 0.3\n*SOLID SECTION, ELSET=EALL, MATERIAL=STEEL\n")
w("*BOUNDARY\nEND_L, 1, 3\nEND_R, 1, 3\nPIN_L, 2, 3\nPIN_R, 2, 3\n")
w("*STEP\n*STATIC\n*DLOAD\n")
pads = [(-1550, -1450, -11.25), (-1250, -1150, -11.25), (1150, 1200, 144.583333),
        (1200, 1250, -122.083333), (1450, 1500, 144.583333), (1500, 1550, -122.083333)]
for x0, x1, p in pads:
    for k in range(nz):
        for i in range(nx):
            if x0 - 1e-6 <= xs[i] and xs[i + 1] <= x1 + 1e-6:
                w(f"{eid(i, ny - 1, k)}, P5, {p}\n")
for name in ("PIN_L", "PIN_R", "END_L", "END_R"):
    w(f"*NODE PRINT, NSET={name}, TOTALS=ONLY\nRF\n")
for name in ("TOP_0", "TOP_P1500", "TOP_M1500"):
    w(f"*NODE PRINT, NSET={name}\nU\n")
w("*END STEP\n")
sys.stderr.write(f"level {level}: {nx} x {ny} x {nz} elements, {(nx + 1) * (ny + 1) * (nz + 1)} nodes, smallest dx {min(q - p for p, q in zip(xs, xs[1:])):.3g}\n")
