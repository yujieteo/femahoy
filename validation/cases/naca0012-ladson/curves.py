"""The curves of the report of naca0012-ladson (scripts/validation-reports.py).

    python3 curves.py OUT a-0.05=FOLDER a4.04=FOLDER a10.12=FOLDER

FOLDER is the output folder of the run of each input. The script writes OUT/lift.csv and
OUT/drag.csv, the lift and the drag against the angle of attack with the values of Table IX of
Ladson (1988). Only the Python standard library.
"""

import sys
import tomllib
from pathlib import Path

CHECKS = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
# The input, the angle, then cl and cd of Table IX of the source. The lift at -0.05 degrees is not
# a check: only the table of derivation.md, "Sources", has it. The other values come from check.toml.
ANGLES = [("a-0.05", -0.05, -0.0126, CHECKS["cd_a0"]["value"]),
          ("a4.04", 4.04, CHECKS["cl_a4"]["value"], CHECKS["cd_a4"]["value"]),
          ("a10.12", 10.12, CHECKS["cl_a10"]["value"], CHECKS["cd_a10"]["value"])]
TEST = "Ladson (1988), NASA TM-4074, Table IX, 80-W grit"


def coefficients(foam):
    """Cl and Cd of the last iteration."""
    lines = (foam / "postProcessing/forceCoeffs/0/coefficient.dat").read_text().splitlines()
    names = lines[[i for i, line in enumerate(lines) if line.startswith("#")][-1]][1:].split()
    values = lines[-1].split()
    return float(values[names.index("Cl")]), float(values[names.index("Cd")])


def main(out, folders):
    out = Path(out)
    runs = dict(f.split("=", 1) for f in folders)
    lift, drag = [], []
    for name, angle, cl_test, cd_test in ANGLES:
        cl, cd = coefficients(Path(runs[name]) / "foam")
        lift.append(f"{angle},{cl:.5f},{cl_test}")
        drag.append(f"{angle},{cd:.6f},{cd_test}")
    (out / "lift.csv").write_text(
        "# title: The lift against the angle of attack\n"
        "# y: cl [1]\n"
        f"# points test: {TEST}\n"
        "# note: The lift of the test at -0.05 degrees is not a check: it is an offset of the test (see Sources).\n"
        "angle of attack [degrees],model,test\n" + "\n".join(lift) + "\n"
    )
    (out / "drag.csv").write_text(
        "# title: The drag against the angle of attack\n"
        "# y: cd [1]\n"
        f"# points test: {TEST}\n"
        "angle of attack [degrees],model,test\n" + "\n".join(drag) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
