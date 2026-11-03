"""The curve of the report of plasticity-sphere (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/history.csv: the radial
displacement u(b) of the outer face against the inner pressure p at each increment of the run
(ccx/job.dat), with the elastic-plastic solution of Hill (1950) in derivation.md. Only the Python
standard library.
"""

import math
import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
E, NU, Y = 210000.0, 0.3, 250.0  # MPa, 1, MPa
A, B = 10.0, 20.0  # the inner and the outer radius, mm
P = 300.0  # the pressure at the end of the step, MPa (job.inp)
P_YIELD = 2 * Y / 3 * (1 - A**3 / B**3)


def radius(p):
    """c, the outer radius of the plastic zone: the root of p = 2Y ln(c/a) + (2Y/3)(1 - c^3/b^3)."""
    lo, hi = A, B
    for _ in range(100):
        c = (lo + hi) / 2
        if 2 * Y * math.log(c / A) + 2 * Y / 3 * (1 - c**3 / B**3) < p:
            lo = c
        else:
            hi = c
    return (lo + hi) / 2


def hill(p):
    """u(b) = (1 - nu) Y c^3 / (E b^2). Below the first yield, c = a and u(b) is linear in p."""
    if p <= P_YIELD:
        return p / P_YIELD * (1 - NU) * Y * A**3 / (E * B * B)
    return (1 - NU) * Y * radius(p) ** 3 / (E * B * B)


def main(out, folders):
    reference = tomllib.loads((HERE / "check.toml").read_text())["u_outer"]["value"]
    if abs(hill(P) / reference - 1) > 1e-5:
        sys.exit(f"curves.py: Hill gives u(b) = {hill(P)} at {P} MPa, check.toml gives {reference}")
    dat = (Path(dict(f.split("=", 1) for f in folders)["job.inp"]) / "ccx/job.dat").read_text()
    model = [
        (round(P * float(t), 6), float(u))
        for t, u in re.findall(r"displacements \(vx,vy,vz\) for set PX and time\s+(\S+)\n\n\s+\d+\s+(\S+)", dat)
    ]
    if not model:
        sys.exit("curves.py: job.dat has no displacement of PX")
    worst = max(model, key=lambda r: abs(r[1] / hill(r[0]) - 1))
    rows = {5.0 * k: "" for k in range(int(P / 5) + 1)} | {P_YIELD: ""} | {p: f"{u:.7g}" for p, u in model}
    (Path(out) / "history.csv").write_text(
        "# title: Outer radial displacement against the inner pressure\n"
        "# y: u(b) [mm]\n"
        "# points model: u1 of the node PX = (20, 0, 0) at each increment, ccx/job.dat of the run\n"
        f"# note: Hill (1950) is the solution of derivation.md. Below the first yield at {P_YIELD:.1f} MPa, "
        "the sphere is elastic: u(b) is linear in p, the formula with c = a. Above it, c is the root of the "
        "pressure equation.\n"
        f"# note: The largest difference of the model is {100 * (worst[1] / hill(worst[0]) - 1):+.2f} %, "
        f"at p = {worst[0]:.0f} MPa.\n"
        "inner pressure p [MPa],model,Hill (1950)\n"
        + "".join(f"{p:.6g},{u},{hill(p):.7g}\n" for p, u in sorted(rows.items()))
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
