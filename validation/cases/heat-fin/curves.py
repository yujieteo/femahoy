"""The curve of the report of heat-fin (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/fin.csv: the temperature
along the edge y = 0, z = 0 of the fin, with the fin theory of Incropera et al. (2007), Table 3.4,
case A. Only the Python standard library.
"""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import vtu  # noqa: E402

# derivation.md, "Model": L = 100 mm, mL = 1, h / mk = 0.025, the base at 100 °C, the air at 20 °C.
L, ML, HMK, TB, TAIR = 100.0, 1.0, 0.025, 100.0, 20.0


def fin(x):
    """Incropera et al. (2007), Table 3.4, case A: the temperature at x of a fin with convection
    at the tip."""
    m = ML / L
    return TAIR + (TB - TAIR) * (math.cosh(m * (L - x)) + HMK * math.sinh(m * (L - x))) / (
        math.cosh(ML) + HMK * math.sinh(ML)
    )


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    step = vtu.read(Path(runs["job.inp"]) / "step-1.vtu")
    edge = sorted((p[0], t) for p, t in zip(step.points, step.point["NT"]) if abs(p[1]) < 1e-9 and abs(p[2]) < 1e-9)
    worst = max(edge, key=lambda e: abs(e[1] - fin(e[0])))
    rows = "".join(f"{x:g},{t:.4f},{fin(x):.4f}\n" for x, t in edge)
    (Path(out) / "fin.csv").write_text(
        "# title: The temperature along the fin\n"
        "# y: T [°C]\n"
        "# points model: the nodes of the edge y = 0, z = 0 of the solid, the coldest line of each section\n"
        "# note: The fin theory is Incropera et al. (2007), Table 3.4, case A, with 1 temperature in each section.\n"
        f"# note: The largest difference of the model from the fin theory is {worst[1] - fin(worst[0]):+.3f} °C, "
        f"at x = {worst[0]:g} mm.\n"
        "x [mm],model,fin theory\n" + rows
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
