"""The curve of the report of fracture-centre-crack (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/opening.csv: K_I from the
opening v of each node of the crack face near the tip, with the reference K_I of check.toml.
Only the Python standard library.
"""

import math
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1]))
import vtu  # noqa: E402

# derivation.md: steel E = 210000 MPa, nu = 0.3, plane strain. The tip is at x = a = 25 mm. The
# 8 rings of the fine mesh go to 5 mm from the tip.
E, NU, TIP, RINGS = 210000.0, 0.3, 25.0, 5.0


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    step = vtu.read(Path(runs["job.inp"]) / "step-1.vtu")
    reference = tomllib.loads((HERE / "check.toml").read_text())["k_i"]["value"]
    e = E / (1 - NU * NU)
    # The crack face is y = 0, x < a. Each node is 3 times in the step file (z = -0.5, 0, 0.5).
    face = sorted({round(TIP - p[0], 6): u[1] for p, u in zip(step.points, step.point["U"])
                   if abs(p[1]) < 1e-9 and 0 < TIP - p[0] <= RINGS + 1e-6}.items())
    rows = "".join(f"{r:.4f},{e / 4 * v * math.sqrt(2 * math.pi / r):.2f},{reference}\n" for r, v in face)
    (Path(out) / "opening.csv").write_text(
        "# title: K_I from the opening of the crack face near the tip\n"
        "# y: K_I [MPa mm^0.5]\n"
        "# note: model is K_I = (E' / 4) v sqrt(2 pi / r) at each node of the crack face, from v = (4 K_I / E') sqrt(r / 2 pi). "
        "This formula is the first term of the field at the tip. It is exact only as r goes to 0.\n"
        "# note: Tada is the formula of Tada (1971) in check.toml. The check gets K_I of the model from the 2 nodes at r = L/4 and L (derivation.md).\n"
        "r = a - x [mm],model,Tada\n" + rows
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
