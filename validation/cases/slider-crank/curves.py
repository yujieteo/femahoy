"""The curves of the report of slider-crank (scripts/validation-reports.py).

    python3 curves.py OUT 60rpm=FOLDER

FOLDER is the output folder of the run of 60rpm. The script writes OUT/torque.csv, the motor
torque of the model and of the closed form against the time, and OUT/torque-error.csv, the
difference of the 2. The closed form is reference.py of this case. In torque.csv the model is
points at each 25th time step (the frames of the 3D view), so that the line of the closed form
shows between them. Only the Python standard library.
"""

import sys
from pathlib import Path

# reference.py is in this folder.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from reference import state, w  # noqa: E402

# The time steps from 1 frame of the 3D view to the next (FRAME of chrono.py).
FRAME = 25


def main(out, folders):
    out = Path(out)
    runs = dict(f.split("=", 1) for f in folders)
    table = Path(runs["60rpm"]) / "chrono/slider-crank.txt"
    rows = [[float(v) for v in line.split()] for line in table.read_text().splitlines() if not line.startswith("#")]
    torque, error = [], []
    for k, (t, _, model) in enumerate(rows):
        exact = state(w * t)[1]
        # Chrono gives the torque of the motor from the first step. The row of t = 0 has 0.
        if t == 0:
            torque.append(f"{t:.4g},,{exact:.9g}")
            continue
        torque.append(f"{t:.4g},{model:.9g},{exact:.9g}" if k % FRAME == 0 else f"{t:.4g},,{exact:.9g}")
        # A log axis needs values above 0.
        error.append(f"{t:.4g},{abs(model - exact):.4g}" if model != exact else f"{t:.4g},")
    (out / "torque.csv").write_text(
        "# title: The motor torque against the time\n"
        "# y: torque [N m]\n"
        "# points model: Chrono, each 25th time step of 1 ms\n"
        "# note: The crank turns at 60 rpm from the dead centre. The checks read the values at t = 1.125 s, "
        "when the crank is at 45 degrees after 2.25 turns.\n"
        "t [s],model,closed form\n" + "\n".join(torque) + "\n"
    )
    (out / "torque-error.csv").write_text(
        "# title: The torque error of the model against the time\n"
        "# y: |model - closed form| [N m]\n"
        "# log: y\n"
        "# note: The start accelerations of the time stepper do not agree with 60 rpm, thus the torque has "
        "a start transient. The tolerance of the check at t = 1.125 s is 1.4e-4 N m.\n"
        "t [s],torque error\n" + "\n".join(error) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
