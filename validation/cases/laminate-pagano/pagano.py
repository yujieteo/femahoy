"""Write mesh.inp and load.inp of 2 square [0/90/0] plates and print their exact values.

Usage: python3 pagano.py [--shell N] [--solid N NZ] [--thin-h H] [--table]

Each plate is simply supported, with the load q0 sin(pi x / a) sin(pi y / a) down on it, and the
model is 1/4 of the plate, 0 <= x, y <= a/2 (local x). See derivation.md.
THIN: a/h = 100, N x N S8R composite shells, the load on the mid-surface. --thin-h sets its h.
THICK: a/h = 4, N x N x 3 NZ C3D20R (NZ in each ply), the load on the top face, at x = 60 to 110.
The script prints the exact value of each check of check.toml at the integration points of its
element set. --table prints the normalized exact values for 5 span ratios, as the papers give them.
"""

import argparse
import math

ET = 7000.0
EL, GLT, GTT, NU = 25.0 * ET, 0.5 * ET, 0.2 * ET, 0.25
A = 100.0
THIN = dict(h=1.0, q0=0.001, x0=0.0)
THICK = dict(h=25.0, q0=1.0, x0=60.0)
# The node and element numbers of THICK start after this offset.
OFFSET = 100000
G = 1.0 / math.sqrt(3.0)


def mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(len(b))) for j in range(len(b[0]))]
            for i in range(len(a))]


def apply(a, v):
    return [sum(x * y for x, y in zip(row, v)) for row in a]


def expm(m):
    """exp(m) by scaling and squaring of the Taylor series."""
    n = len(m)
    norm = max(sum(abs(x) for x in row) for row in m)
    s = max(0, math.ceil(math.log2(norm)) + 1) if norm > 0 else 0
    b = [[x / 2**s for x in row] for row in m]
    e = [[float(i == j) for j in range(n)] for i in range(n)]
    term = [row[:] for row in e]
    for k in range(1, 30):
        term = [[x / k for x in row] for row in mul(term, b)]
        e = [[x + y for x, y in zip(r, t)] for r, t in zip(e, term)]
    for _ in range(s):
        e = mul(e, e)
    return e


def inverse3(m):
    (a, b, c), (d, e, f), (g, h, i) = m
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    cof = [[e * i - f * h, c * h - b * i, b * f - c * e],
           [f * g - d * i, a * i - c * g, c * d - a * f],
           [d * h - e * g, b * g - a * h, a * e - b * d]]
    return [[x / det for x in row] for row in cof]


def stiffness(angle):
    """C11, C12, C13, C22, C23, C33, C44 (yz), C55 (xz), C66 (xy) of a ply at 0 or 90 degrees."""
    s = [[1 / EL, -NU / EL, -NU / EL], [-NU / EL, 1 / ET, -NU / ET], [-NU / EL, -NU / ET, 1 / ET]]
    c = inverse3(s)
    c11, c12, c13, c22, c23, c33 = c[0][0], c[0][1], c[0][2], c[1][1], c[1][2], c[2][2]
    c44, c55, c66 = GTT, GLT, GLT
    if angle == 90:
        c11, c22, c13, c23, c44, c55 = c22, c11, c23, c13, c55, c44
    return c11, c12, c13, c22, c23, c33, c44, c55, c66


class Exact:
    """The 3D elasticity solution of the plate of side A and thickness h, load q0 down on the top.

    u = U(z) cos(px) sin(py), v = V(z) sin(px) cos(py), w = W(z) sin(px) sin(py), with
    tau_xz = X cos sin, tau_yz = Y sin cos, sigma_z = Z sin sin, p = pi / A. The state
    s = (U, V, W, X, Y, Z) solves s' = M s in each ply. Each ply is h/3 thick.
    """

    def __init__(self, h, q0):
        self.p = p = math.pi / A
        self.plies = []
        for k, angle in enumerate((0, 90, 0)):
            c11, c12, c13, c22, c23, c33, c44, c55, c66 = stiffness(angle)
            wz = [p * c13 / c33, p * c23 / c33, 0, 0, 0, 1 / c33]
            sx = [-p * c11 + c13 * wz[0], -p * c12 + c13 * wz[1], 0, 0, 0, c13 * wz[5]]
            sy = [-p * c12 + c23 * wz[0], -p * c22 + c23 * wz[1], 0, 0, 0, c23 * wz[5]]
            txy = [p * c66, p * c66, 0, 0, 0, 0]
            m = [[0, 0, -p, 1 / c55, 0, 0], [0, 0, -p, 0, 1 / c44, 0], wz,
                 [p * t - p * x for t, x in zip(txy, sx)],
                 [p * t - p * y for t, y in zip(txy, sy)], [0, 0, 0, p, p, 0]]
            z0 = -h / 2 + k * h / 3
            self.plies.append((z0, z0 + h / 3, m, sx, sy, txy))
        whole = [[float(i == j) for j in range(6)] for i in range(6)]
        for z0, z1, m, *_ in self.plies:
            whole = mul(expm([[x * (z1 - z0) for x in row] for row in m]), whole)
        # Bottom: X = Y = Z = 0. Top: X = Y = 0, Z = -q0.
        k = inverse3([row[:3] for row in whole[3:]])
        self.bottom = apply(k, [0.0, 0.0, -q0]) + [0.0, 0.0, 0.0]

    def state(self, z):
        """The state and the ply at z, from the bottom up. At an interface, the lower ply."""
        s = self.bottom
        for z0, z1, m, sx, sy, txy in self.plies:
            end = min(z, z1)
            s = apply(expm([[x * (end - z0) for x in row] for row in m]), s)
            if z <= z1 + 1e-12:
                return s, (sx, sy, txy)
        raise ValueError(z)

    def stress(self, x, y, z):
        """sigma_xx, sigma_yy, sigma_zz, tau_xy, tau_xz, tau_yz at the local point (x, y, z)."""
        s, (sx, sy, txy) = self.state(z)
        sn, cs = math.sin(self.p * x), math.cos(self.p * x)
        sm, cm = math.sin(self.p * y), math.cos(self.p * y)
        dot = lambda r: sum(a * b for a, b in zip(r, s))
        return [dot(sx) * sn * sm, dot(sy) * sn * sm, s[5] * sn * sm, dot(txy) * cs * cm,
                s[3] * cs * sm, s[4] * sn * cm]

    def w(self, z=0.0):
        return self.state(z)[0][2]


def s8(xi, eta):
    """The shape functions of the 8-node face: 4 corners, then 4 mid-side nodes."""
    n = [0.25 * (1 + xi * a) * (1 + eta * b) * (xi * a + eta * b - 1)
         for a, b in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    return n + [0.5 * (1 - xi * xi) * (1 - eta), 0.5 * (1 + xi) * (1 - eta * eta),
                0.5 * (1 - xi * xi) * (1 + eta), 0.5 * (1 - xi) * (1 - eta * eta)]


GAUSS4 = [(-0.8611363115940526, 0.3478548451374538), (-0.3399810435848563, 0.6521451548625461),
          (0.3399810435848563, 0.6521451548625461), (0.8611363115940526, 0.3478548451374538)]


def face_loads(x0, x1, y0, y1, q0):
    """The nodal forces of the load q0 sin sin on the face [x0, x1] x [y0, y1] (local x)."""
    f = [0.0] * 8
    jac = (x1 - x0) * (y1 - y0) / 4
    for xi, wx in GAUSS4:
        for eta, wy in GAUSS4:
            x, y = x0 + (1 + xi) / 2 * (x1 - x0), y0 + (1 + eta) / 2 * (y1 - y0)
            q = q0 * math.sin(math.pi * x / A) * math.sin(math.pi * y / A)
            for k, nk in enumerate(s8(xi, eta)):
                f[k] += nk * q * wx * wy * jac
    return f


def quad(i, j):
    """The (i, j) grid points of the 8-node face with its first corner at (i, j)."""
    return [(i, j), (i + 2, j), (i + 2, j + 2), (i, j + 2),
            (i + 1, j), (i + 2, j + 1), (i + 1, j + 2), (i, j + 1)]


def nset(name, ids):
    return [f"*NSET, NSET={name}"] + [", ".join(map(str, ids[k:k + 16])) + ","
                                      for k in range(0, len(ids), 16)]


def elset(name, ids):
    return [f"*ELSET, ELSET={name}"] + [", ".join(map(str, ids[k:k + 16])) + ","
                                        for k in range(0, len(ids), 16)]


def thin(n):
    """The mesh lines, the load lines and the integration points of the checks of THIN."""
    m, d = 2 * n + 1, A / 2 / (2 * n)
    nid = lambda i, j: j * m + i + 1
    mesh = ["*NODE, NSET=NALL"]
    mesh += [f"{nid(i, j)}, {i * d:.12g}, {j * d:.12g}, 0.0" for j in range(m) for i in range(m)
             if not (i % 2 and j % 2)]
    mesh.append("*ELEMENT, TYPE=S8R, ELSET=THIN")
    forces = {}
    for jj in range(n):
        for ii in range(n):
            nodes = [nid(i, j) for i, j in quad(2 * ii, 2 * jj)]
            mesh.append(f"{jj * n + ii + 1}, " + ", ".join(map(str, nodes)))
            x0, y0 = 2 * ii * d, 2 * jj * d
            for k, f in zip(nodes, face_loads(x0, x0 + 2 * d, y0, y0 + 2 * d, THIN["q0"])):
                forces[k] = forces.get(k, 0.0) + f
    edge = lambda keep: [nid(i, j) for j in range(m) for i in range(m)
                         if not (i % 2 and j % 2) and keep(i, j)]
    mesh += nset("THIN_SSX", edge(lambda i, j: i == 0))
    mesh += nset("THIN_SSY", edge(lambda i, j: j == 0))
    mesh += nset("THIN_SYMX", edge(lambda i, j: i == m - 1))
    mesh += nset("THIN_SYMY", edge(lambda i, j: j == m - 1))
    mesh += nset("THIN_CENTRE", [nid(m - 1, m - 1)])
    mesh += elset("THIN_BOTTOM", [n * n])
    load = [f"{k}, 3, {-f:.12e}" for k, f in sorted(forces.items())]
    # The centre element: 2 x 2 points in the plane, 2 points across each of the 3 layers.
    h = THIN["h"]
    points = [(A / 2 - d * (1 - a * G), A / 2 - d * (1 - b * G),
               -h / 2 + h / 3 * (k + (1 + c * G) / 2))
              for k in range(3) for c in (-1, 1) for b in (-1, 1) for a in (-1, 1)]
    return mesh, load, points


def thick(n, nz):
    """The mesh lines, the load lines and the integration points of the checks of THICK."""
    m, mz = 2 * n + 1, 6 * nz + 1
    d, h, x0 = A / 2 / (2 * n), THICK["h"], THICK["x0"]
    dz = h / (mz - 1)
    nid = lambda i, j, k: OFFSET + (k * m + j) * m + i + 1
    keep = lambda i, j, k: i % 2 + j % 2 + k % 2 <= 1
    grid = [(i, j, k) for k in range(mz) for j in range(m) for i in range(m) if keep(i, j, k)]
    mesh = ["*NODE, NSET=NALL"]
    mesh += [f"{nid(i, j, k)}, {x0 + i * d:.12g}, {j * d:.12g}, {-h / 2 + k * dz:.12g}"
             for i, j, k in grid]
    mesh.append("*ELEMENT, TYPE=C3D20R, ELSET=THICK")
    eid = lambda ii, jj, kk: OFFSET + (kk * n + jj) * n + ii + 1
    plies = {0: [], 90: []}
    for kk in range(3 * nz):
        for jj in range(n):
            for ii in range(n):
                face = quad(2 * ii, 2 * jj)
                k = 2 * kk
                # C3D20: 4 corners at the bottom, 4 at the top, then the mid-side nodes of the
                # bottom, of the top, and of the 4 vertical edges.
                nodes = [nid(i, j, k + dk) for dk in (0, 2) for i, j in face[:4]]
                nodes += [nid(i, j, k + dk) for dk in (0, 2) for i, j in face[4:]]
                nodes += [nid(i, j, k + 1) for i, j in face[:4]]
                mesh.append(f"{eid(ii, jj, kk)}, " + ", ".join(map(str, nodes[:15])) + ",")
                mesh.append(", ".join(map(str, nodes[15:])))
                plies[90 if kk // nz == 1 else 0].append(eid(ii, jj, kk))
    mesh += elset("THICK_0", plies[0]) + elset("THICK_90", plies[90])
    face = lambda test: [nid(i, j, k) for i, j, k in grid if test(i, j, k)]
    mesh += nset("THICK_SSX", face(lambda i, j, k: i == 0))
    mesh += nset("THICK_SSY", face(lambda i, j, k: j == 0))
    mesh += nset("THICK_SYMX", face(lambda i, j, k: i == m - 1))
    mesh += nset("THICK_SYMY", face(lambda i, j, k: j == m - 1))
    mesh += nset("THICK_CENTRE", [nid(m - 1, m - 1, (mz - 1) // 2)])
    # The element at the centre on the bottom face, and the element of the core at the edge x = 0
    # with its centre on (or just above) the mid-plane.
    mid = 3 * nz // 2
    mesh += elset("THICK_BOTTOM", [eid(n - 1, n - 1, 0)])
    mesh += elset("THICK_EDGE", [eid(0, n - 1, mid)])
    forces = {}
    for jj in range(n):
        for ii in range(n):
            nodes = [nid(i, j, mz - 1) for i, j in quad(2 * ii, 2 * jj)]
            xa, ya = 2 * ii * d, 2 * jj * d
            for k, f in zip(nodes, face_loads(xa, xa + 2 * d, ya, ya + 2 * d, THICK["q0"])):
                forces[k] = forces.get(k, 0.0) + f
    load = [f"{k}, 3, {-f:.12e}" for k, f in sorted(forces.items())]
    # The points of each element in the order of ccx: x first, then y, then z.
    cube = [(a, b, c) for c in (-1, 1) for b in (-1, 1) for a in (-1, 1)]
    bottom = [(A / 2 - d * (1 - a * G), A / 2 - d * (1 - b * G), -h / 2 + dz * (1 + c * G))
              for a, b, c in cube]
    z_mid = -h / 2 + 2 * mid * dz + dz
    edge = [(d * (1 + a * G), A / 2 - d * (1 - b * G), z_mid + dz * c * G) for a, b, c in cube]
    return mesh, load, bottom, edge


def table():
    """The normalized exact values of the papers, and of the classical laminate theory (CLT)."""
    print("a/h  w(0)    w(h/2)  sx(h/2)  sx(-h/2) sy(h/6) txz(0)  tyz(0)  txy(h/2) | CLT w  CLT sx")
    for s in (4, 10, 20, 50, 100):
        h = A / s
        ex = Exact(h, 1.0)
        c = A / 2
        nw = lambda w: 100 * ET * h**3 * abs(w) / A**4
        sx_top = ex.stress(c, c, h / 2)[0] / s**2
        sx_bot = ex.stress(c, c, -h / 2)[0] / s**2
        sy = ex.stress(c, c, h / 6)[1] / s**2
        txz = ex.stress(0, c, 0)[4] / s
        tyz = ex.stress(c, 0, 0)[5] / s
        txy = ex.stress(0, 0, h / 2)[3] / s**2
        # CLT: w = q0 a^4 / (pi^4 (D11 + 2 (D12 + 2 D66) + D22)) for the square plate.
        q = [stiffness(0), stiffness(90)]
        red = [(c11 - c13**2 / c33, c12 - c13 * c23 / c33, c22 - c23**2 / c33, c66)
               for c11, c12, c13, c22, c23, c33, c44, c55, c66 in q]
        z = [(-h / 2, -h / 6, 0), (-h / 6, h / 6, 1), (h / 6, h / 2, 0)]
        dmat = [sum(red[k][i] * (z1**3 - z0**3) / 3 for z0, z1, k in z) for i in range(4)]
        d11, d12, d22, d66 = dmat
        w_clt = A**4 / (math.pi**4 * (d11 + 2 * (d12 + 2 * d66) + d22))
        kx = (math.pi / A) ** 2 * w_clt
        sx_clt = red[0][0] * kx * h / 2 + red[0][1] * kx * h / 2
        print(f"{s:3d}  {nw(ex.w()):.4f}  {nw(ex.w(h / 2)):.4f}  {abs(sx_top):.4f}   "
              f"{abs(sx_bot):.4f}   {abs(sy):.4f}  {abs(txz):.4f}  {abs(tyz):.4f}  "
              f"{abs(txy):.5f} | {nw(w_clt):.4f} {sx_clt / s**2:.4f}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--shell", type=int, default=8, help="N: N x N S8R in THIN")
    parser.add_argument("--solid", type=int, nargs=2, default=(8, 3), help="N NZ of THICK")
    parser.add_argument("--thin-h", type=float, default=1.0, help="h of THIN, in mm")
    parser.add_argument("--table", action="store_true")
    args = parser.parse_args()
    THIN["h"] = args.thin_h
    if args.table:
        table()
        return
    mesh_a, load_a, points_a = thin(args.shell)
    mesh_b, load_b, bottom, edge = thick(*args.solid)
    with open("mesh.inp", "w") as f:
        f.write("** units: SI-mm\n** Written by pagano.py. See derivation.md.\n")
        f.write("\n".join(mesh_a + mesh_b) + "\n")
    with open("load.inp", "w") as f:
        f.write("*CLOAD\n" + "\n".join(load_a + load_b) + "\n")
    a, b = Exact(THIN["h"], THIN["q0"]), Exact(THICK["h"], THICK["q0"])
    print(f"w_thin    {a.w():.6g}")
    print(f"sx_thin   {max(a.stress(*p)[0] for p in points_a):.6g}")
    print(f"w_thick   {b.w():.6g}")
    print(f"sx_thick  {max(b.stress(*p)[0] for p in bottom):.6g}")
    print(f"txz_thick {sum(b.stress(*p)[4] for p in edge) / len(edge):.6g}")


if __name__ == "__main__":
    main()
