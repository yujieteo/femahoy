"""Write the OpenFOAM cases of an airfoil check: 1 folder for each angle of attack and flap.

Usage, in the repository root: python3 validation/airfoil.py CASE [LEVEL [ITERATIONS [OUT]]]

CASE is a key of CASES. The defaults (level 2, the iterations of the case, validation/cases/CASE)
write the folders that the check.toml of the case names. Level 1 is the coarsest mesh. Each level
doubles the cells in each direction and halves the first cell height at the wall. The
derivation.md of each case gives its mesh study.

The mesh is a 2D C-grid of 12 blocks, 1 cell deep. The inner layer of blocks is D thick, and
its lines leave the wall along the normal. The outer layer goes out to a circle of radius R.
Above and below the wake, the far field is straight to the outlet.
"""
import math
import os
import sys

R = 500.0                      # radius of the far field circle, in chords
D = 1.0                        # thickness of the inner layer
XS = 0.3                       # split of the front and aft blocks, near the maximum thickness
PHI_13 = 100.0                 # angle of the far field end of the line from the upper split
X_OUT = 500.0                  # outlet line
M = 48                         # spline points on each part of the surface


def half_thickness(x):
    """TMR NACA 0012 with a sharp trailing edge at x = 1 (chord 1)."""
    if x >= 1:
        return 0.0
    return 0.594689181 * (0.298222773 * math.sqrt(x) - 0.127125232 * x
                          - 0.357907906 * x**2 + 0.291984971 * x**3
                          - 0.105174606 * x**4)


def naca0012(x):
    """The upper and the lower point of the NACA 0012 of TMR at the station x."""
    t = half_thickness(x)
    return (x, t), (x, -t)


def mean_line_23012(x):
    """The NACA 230 mean line (m = 0.2025, k1 = 15.957) at x: the height and the slope."""
    m, k1 = 0.2025, 15.957
    if x < m:
        return k1 / 6 * (x**3 - 3 * m * x**2 + m**2 * (3 - m) * x), \
            k1 / 6 * (3 * x**2 - 6 * m * x + m**2 * (3 - m))
    return k1 / 6 * m**3 * (1 - x), -k1 / 6 * m**3


def naca23012(flap):
    """The surface of the NACA 23012 with a 0.20c plain flap down `flap` degrees (a negative flap
    is up): a function from the station x to the upper and the lower point. The thickness is half_thickness, normal
    to the mean line. The hinge is on the mean line at x = 0.8. Each point at x turns about the
    hinge by flap * S((x - 0.8) / W), with S a smooth step from 0 to 1. Thus the flap aft of
    0.8 + W turns as a rigid body, and the surface stays smooth across the sealed hinge."""
    hinge = (0.8, mean_line_23012(0.8)[0])
    W = 0.05

    def turn(p, x):
        s = min(1.0, max(0.0, (x - hinge[0]) / W))
        a = -math.radians(flap) * s * s * (3 - 2 * s)
        dx, dy = p[0] - hinge[0], p[1] - hinge[1]
        return (hinge[0] + dx * math.cos(a) - dy * math.sin(a),
                hinge[1] + dx * math.sin(a) + dy * math.cos(a))

    def surface(x):
        yc, slope = mean_line_23012(x)
        t = half_thickness(x)
        s, c = math.sin(math.atan(slope)), math.cos(math.atan(slope))
        return turn((x - t * s, yc + t * c), x), turn((x + t * s, yc - t * c), x)
    return surface


def ratio(first, length, n):
    """The ratio r of n cells in a geometric series with the first cell `first` on `length`."""
    def total(r):
        return first * n if abs(r - 1) < 1e-12 else first * (r**n - 1) / (r - 1)
    lo, hi = 0.2, 3.0
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        lo, hi = (mid, hi) if total(mid) < length else (lo, mid)
    return 0.5 * (lo + hi)


def fractions(expansion, n):
    """The fractions of the length at the n + 1 points of a geometric grading."""
    q = expansion ** (1 / (n - 1))
    if abs(q - 1) < 1e-12:
        return [k / n for k in range(n + 1)]
    return [(q**k - 1) / (q**n - 1) for k in range(n + 1)]


def length(pts):
    return sum(math.dist(a, b) for a, b in zip(pts, pts[1:]))


def at_length(path, s):
    """The point at the arc length s on the polyline `path`, and the unit tangent there."""
    run = 0.0
    for a, b in zip(path, path[1:]):
        step = math.dist(a, b)
        if run + step >= s or b is path[-1]:
            t = min(1.0, (s - run) / step)
            point = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]))
            return point, ((b[0] - a[0]) / step, (b[1] - a[1]) / step)
        run += step


def block_mesh_dict(title, surface, level):
    """The text of system/blockMeshDict, and the number of cells. `surface` gives the upper and
    the lower point at the station x. Its LE (x = 0) is at (0, 0)."""
    k = 2 ** (level - 1)
    n_front, n_aft, n_wake = 16 * k, 48 * k, 32 * k   # n_front is for each half of the front
    n_in, n_out = 32 * k, 16 * k
    h1 = 8e-6 / k                                      # first cell height at the wall
    ds_le = ds_te = 4e-3 / k                           # surface spacing at the LE and the TE

    upper, lower = (lambda x: surface(x)[0]), (lambda x: surface(x)[1])
    ts = 0.5 * (upper(XS)[1] - lower(XS)[1])
    # The circle of the inner front edge goes through (-D, 0) and (XS, ts + D).
    xc = (XS**2 + ts**2 + 2 * ts * D) / (2 * (XS + D))
    rho = xc + D

    def on_circle(radius, deg):
        return (xc + radius * math.cos(math.radians(deg)), radius * math.sin(math.radians(deg)))

    theta_a = math.degrees(math.atan2(ts + D, XS - xc))

    # Surface: the upper front from the LE to the split, and the upper aft to the TE.
    stations_front = [XS * (i / M) ** 2 for i in range(M + 1)]
    stations_aft = [XS + (1 - XS) * i / M for i in range(M + 1)]
    front = [upper(x) for x in stations_front]
    aft = [upper(x) for x in stations_aft]
    e_f = ratio(ds_le, length(front), n_front) ** (n_front - 1)
    e_aft = ds_te / (ds_le * e_f)

    # The inner arc: each point where the normal of its surface point meets the circle.
    want = []
    for f in fractions(e_f, n_front):
        (x, y), (tx, ty) = at_length(front, f * length(front))
        nx, ny = -ty, tx
        dx, dy = x - xc, y
        b = dx * nx + dy * ny
        t = -b + math.sqrt(b * b - (dx * dx + dy * dy - rho * rho))
        ang = math.degrees(math.atan2(y + t * ny, x + t * nx - xc)) % 360
        want.append((180.0 - ang) / (180.0 - theta_a))
    e_arc = min((10 ** (j / 200) for j in range(-800, 801)),
                key=lambda e: sum((a - b) ** 2 for a, b in zip(fractions(e, n_front), want)))

    # Normal direction: 1 geometric series from the wall to the far field.
    r_in = ratio(h1, D, n_in)
    e_in = r_in ** (n_in - 1)
    e_out = ratio(h1 * r_in**n_in, R - rho, n_out) ** (n_out - 1)

    # Far field: P is on the circle above the TE, and the far field is straight from P to the
    # outlet. The cells are uniform above the aft blocks, and they grow from that width to the
    # front and to the outlet.
    py = math.sqrt(R * R - (1.0 - xc) ** 2)
    theta_p = math.degrees(math.atan2(py, 1.0 - xc))
    w_aft = R * math.radians(PHI_13 - theta_p) / n_aft
    e_o1 = ratio(w_aft, R * math.radians(180.0 - PHI_13), n_front) ** (n_front - 1)
    e_o3 = ratio(w_aft, X_OUT - 1.0, n_wake) ** (n_wake - 1)

    # Wake: from the TE spacing to the outlet. The outlet keeps the distribution of the wall.
    e_w = ratio(ds_te, X_OUT - 1, n_wake) ** (n_wake - 1)
    e_o3r = ratio(h1 * r_in**n_in, py - D, n_out) ** (n_out - 1)

    # The wake cut goes straight downstream from the TE. A cut at a small angle to the x axis
    # gives OpenFOAM wrong centres of its thin and long cells.
    fu = on_circle(R, PHI_13)
    te = upper(1.0)
    pts = [
        te, upper(XS), lower(XS), (0.0, 0.0),                   # 0 TE, 1 2 splits, 3 LE
        (XS, ts + D), (XS, -ts - D), (-D, 0.0),                 # 4 5 inner arc ends, 6 inner LE
        (1.0, D), (1.0, -D),                                    # 7 8 above and below the TE
        (X_OUT, te[1]), (X_OUT, D), (X_OUT, -D),                # 9 10 11 outlet of the inner wake
        (xc - R, 0.0), fu, (fu[0], -fu[1]),                     # 12 13 14 far field front
        (1.0, py), (1.0, -py), (X_OUT, py), (X_OUT, -py),       # 15 16 P, 17 18 outlet corners
    ]
    n = len(pts)
    out = []
    w = out.append
    w("FoamFile { version 2.0; format ascii; class dictionary; object blockMeshDict; }\n\n")
    w(f"// Written by validation/airfoil.py: {title} C-grid of level {level}.\n\nscale 1;\n\n")
    w("vertices\n(\n")
    for z in (0.0, 1.0):
        for x, y in pts:
            w(f"    ({x:.10g} {y:.10g} {z:g})\n")
    w(");\n\nblocks\n(\n")

    def block(name, v, n1, n2, inner, outer, left, right):
        """x1 runs from v0 to v1 (inner edge) and from v3 to v2 (outer edge). x2 runs from v0 to
        v3 (left edge) and from v1 to v2 (right edge). Each grading is last cell / first cell."""
        a, b, c, d = v
        verts = f"{a} {b} {c} {d} {a + n} {b + n} {c + n} {d + n}"
        g = [inner, outer, outer, inner, left, right, right, left, 1, 1, 1, 1]
        grading = " ".join(f"{x:.10g}" for x in g)
        w(f"    hex ({verts}) {name} ({n1} {n2} 1) edgeGrading ({grading})\n")

    block("frontLower", (2, 3, 6, 5), n_front, n_in, 1 / e_f, 1 / e_arc, e_in, e_in)
    block("frontUpper", (3, 1, 4, 6), n_front, n_in, e_f, e_arc, e_in, e_in)
    block("aftUpper", (1, 0, 7, 4), n_aft, n_in, e_aft, 1, e_in, e_in)
    block("aftLower", (0, 2, 5, 8), n_aft, n_in, 1 / e_aft, 1, e_in, e_in)
    block("wakeUpper", (0, 9, 10, 7), n_wake, n_in, e_w, e_w, e_in, e_in)
    block("wakeLower", (9, 0, 8, 11), n_wake, n_in, 1 / e_w, 1 / e_w, e_in, e_in)
    block("outerFrontLower", (5, 6, 12, 14), n_front, n_out, 1 / e_arc, e_o1, e_out, e_out)
    block("outerFrontUpper", (6, 4, 13, 12), n_front, n_out, e_arc, 1 / e_o1, e_out, e_out)
    block("outerAftUpper", (4, 7, 15, 13), n_aft, n_out, 1, 1, e_out, e_out)
    block("outerAftLower", (8, 5, 14, 16), n_aft, n_out, 1, 1, e_out, e_out)
    block("outerWakeUpper", (7, 10, 17, 15), n_wake, n_out, e_w, e_o3, e_out, e_o3r)
    block("outerWakeLower", (11, 8, 16, 18), n_wake, n_out, 1 / e_w, 1 / e_o3, e_o3r, e_out)
    w(");\n\nedges\n(\n")

    def spline(a, b, p):
        for z, off in ((0.0, 0), (1.0, n)):
            body = " ".join(f"({x:.10g} {y:.10g} {z:g})" for x, y in p)
            w(f"    spline {a + off} {b + off} ({body})\n")

    def arc(a, b, radius, deg):
        x, y = on_circle(radius, deg)
        for z, off in ((0.0, 0), (1.0, n)):
            w(f"    arc {a + off} {b + off} ({x:.10g} {y:.10g} {z:g})\n")

    spline(3, 1, front[1:-1])
    spline(1, 0, aft[1:-1])
    spline(0, 2, [lower(x) for x in reversed(stations_aft[1:-1])])
    spline(2, 3, [lower(x) for x in reversed(stations_front[1:-1])])
    mid_a = 0.5 * (180.0 + theta_a)
    arc(6, 4, rho, mid_a)
    arc(5, 6, rho, -mid_a)
    arc(12, 13, R, 0.5 * (180.0 + PHI_13))
    arc(14, 12, R, -0.5 * (180.0 + PHI_13))
    arc(13, 15, R, 0.5 * (PHI_13 + theta_p))
    arc(16, 14, R, -0.5 * (PHI_13 + theta_p))
    w(");\n\nboundary\n(\n")

    def faces(pairs):
        return " ".join(f"({a} {b} {b + n} {a + n})" for a, b in pairs)

    w(f"    wall {{ type wall; faces ({faces([(2, 3), (3, 1), (1, 0), (0, 2)])}); }}\n")
    far = [(14, 12), (12, 13), (13, 15), (15, 17), (17, 10), (10, 9), (9, 11), (11, 18), (18, 16),
           (16, 14)]
    w(f"    farfield {{ type patch; faces ({faces(far)}); }}\n")
    quads = [(2, 3, 6, 5), (3, 1, 4, 6), (1, 0, 7, 4), (0, 2, 5, 8), (0, 9, 10, 7), (9, 0, 8, 11),
             (5, 6, 12, 14), (6, 4, 13, 12), (4, 7, 15, 13), (8, 5, 14, 16), (7, 10, 17, 15),
             (11, 8, 16, 18)]
    sides = " ".join(f"({a} {d} {c} {b}) ({a + n} {b + n} {c + n} {d + n})" for a, b, c, d in quads)
    w(f"    frontAndBack {{ type empty; faces ({sides}); }}\n);\n")
    return "".join(out), (2 * n_front + 2 * n_aft + 2 * n_wake) * (n_in + n_out)


def header(cls, obj):
    return f"FoamFile {{ version 2.0; format ascii; class {cls}; object {obj}; }}\n\n"


def case_files(alpha, iterations, nu, cofr):
    """The files of the case at the angle `alpha` (degrees), except blockMeshDict. CmPitch is
    about the point `cofr`, positive nose up."""
    a = math.radians(alpha)
    ux, uy = math.cos(a), math.sin(a)
    # Free stream turbulence of the SA model: nuTilda = 3 nu, and nut = nuTilda fv1.
    nutilda = 3.0 * nu
    nut = nutilda * 3.0**3 / (3.0**3 + 7.1**3)
    files = {}
    files["system/controlDict"] = header("dictionary", "controlDict") + f"""\
application     simpleFoam;
startFrom       startTime;
startTime       0;
stopAt          endTime;
endTime         {iterations};
deltaT          1;
writeControl    timeStep;
writeInterval   {iterations};
writeFormat     ascii;
writePrecision  10;
timePrecision   6;
runTimeModifiable false;

functions
{{
    forceCoeffs
    {{
        type            forceCoeffs;
        libs            (forces);
        writeControl    timeStep;
        writeInterval   1;
        patches         (wall);
        rho             rhoInf;
        rhoInf          1;
        magUInf         1;
        lRef            1;
        Aref            1;
        liftDir         ({-uy:.12g} {ux:.12g} 0);
        dragDir         ({ux:.12g} {uy:.12g} 0);
        CofR            ({cofr[0]:g} {cofr[1]:g} 0);
    }}
}}
"""
    files["system/fvSchemes"] = header("dictionary", "fvSchemes") + """\
ddtSchemes { default steadyState; }
gradSchemes
{
    default         Gauss linear;
    grad(U)         cellLimited Gauss linear 1;
    grad(nuTilda)   cellLimited Gauss linear 1;
}
divSchemes
{
    default         none;
    div(phi,U)      bounded Gauss linearUpwind grad(U);
    div(phi,nuTilda) bounded Gauss linearUpwind grad(nuTilda);
    div((nuEff*dev2(T(grad(U))))) Gauss linear;
}
laplacianSchemes { default Gauss linear corrected; }
interpolationSchemes { default linear; }
snGradSchemes { default corrected; }
wallDist { method meshWave; }
"""
    files["system/fvSolution"] = header("dictionary", "fvSolution") + """\
solvers
{
    p
    {
        solver          GAMG;
        smoother        GaussSeidel;
        tolerance       1e-10;
        relTol          0.05;
    }
    "(U|nuTilda)"
    {
        solver          smoothSolver;
        smoother        GaussSeidel;
        nSweeps         2;
        tolerance       1e-12;
        relTol          0.1;
    }
}
SIMPLE
{
    consistent      yes;
    nNonOrthogonalCorrectors 0;
}
relaxationFactors
{
    equations { U 0.9; nuTilda 0.9; }
}
"""
    files["constant/transportProperties"] = header("dictionary", "transportProperties") + f"""\
transportModel  Newtonian;
nu              {nu:.10g};
"""
    files["constant/turbulenceProperties"] = header("dictionary", "turbulenceProperties") + """\
simulationType  RAS;
RAS
{
    RASModel        SpalartAllmaras;
    turbulence      on;
    printCoeffs     on;
}
"""

    def field(name, cls, dims, internal, farfield, wall):
        return header(cls, name) + f"""\
dimensions      {dims};
internalField   uniform {internal};
boundaryField
{{
    farfield {{ {farfield} }}
    wall {{ {wall} }}
    frontAndBack {{ type empty; }}
}}
"""
    u = f"({ux:.12g} {uy:.12g} 0)"
    files["0/U"] = field("U", "volVectorField", "[0 1 -1 0 0 0 0]", u,
                         f"type freestreamVelocity; freestreamValue uniform {u};", "type noSlip;")
    files["0/p"] = field("p", "volScalarField", "[0 2 -2 0 0 0 0]", "0",
                         "type freestreamPressure; freestreamValue uniform 0;",
                         "type zeroGradient;")
    files["0/nuTilda"] = field("nuTilda", "volScalarField", "[0 2 -1 0 0 0 0]", f"{nutilda:.10g}",
                               f"type freestream; freestreamValue uniform {nutilda:.10g};",
                               "type fixedValue; value uniform 0;")
    files["0/nut"] = field("nut", "volScalarField", "[0 2 -1 0 0 0 0]", f"{nut:.10g}",
                           f"type freestream; freestreamValue uniform {nut:.10g};",
                           "type nutLowReWallFunction; value uniform 0;")
    return files


# The folders of each case: the angle of attack in degrees and the surface of each folder, the
# Reynolds number, and the point of CmPitch. The validation/cases/<case>/derivation.md files give
# the sources.
CASES = {
    "naca0012-ladson": {
        "title": "NACA 0012",
        # The angles of Ladson (1988) Table IX, and Re = 6 million as TMR.
        "runs": {f"a{a:g}": (a, naca0012) for a in (-0.05, 4.04, 10.12)},
        "re": 6.0e6,
        "cofr": (0.25, 0.0),
        "iterations": 1500,
    },
    "naca23012-flap": {
        "title": "NACA 23012 with flap",
        # Flap 10 degrees up, 0 and 10 degrees down at 0 degrees, and the effective Reynolds
        # number of Abbott and Greenberg (1939) Table I. The point is the a.c. of the airfoil
        # without flap.
        "runs": {f"d{d}": (0.0, naca23012(d)) for d in (-10, 0, 10)},
        "re": 8.4e6,
        "cofr": (0.238, 0.07),
        "iterations": 1000,
    },
}


def main():
    name = sys.argv[1]
    case = CASES[name]
    level = int(sys.argv[2]) if len(sys.argv) > 2 else 2
    iterations = int(sys.argv[3]) if len(sys.argv) > 3 else case["iterations"]
    here = os.path.dirname(os.path.abspath(__file__))
    out = sys.argv[4] if len(sys.argv) > 4 else os.path.join(here, "cases", name)
    for folder, (alpha, surface) in case["runs"].items():
        mesh, cells = block_mesh_dict(case["title"], surface, level)
        files = case_files(alpha, iterations, 1.0 / case["re"], case["cofr"])
        files["system/blockMeshDict"] = mesh
        for path, text in files.items():
            full = os.path.join(out, folder, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w") as f:
                f.write(text)
    print(f"{name} level {level}: {cells} cells, {iterations} iterations, {list(case['runs'])}")


main()
