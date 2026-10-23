"""The curves of the report of laminate-pagano (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes 2 curves of the thick plate
THICK (a/h = 4) through the thickness, with the exact solution of Pagano (1970) from pagano.py:
OUT/sx.csv, sigma_xx at the centre of the plate, and OUT/txz.csv, tau_xz at the middle of the
simply supported edge x = 0. Only the Python standard library.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE.parents[1]), str(HERE)]
import vtu  # noqa: E402
from pagano import A, THICK, Exact  # noqa: E402

# sigma_xx and tau_xz: the components of S in a step file (S11, S22, S33, S12, S23, S13), and of
# Exact.stress of pagano.py.
S11, S13 = 0, 5
SXX, TXZ = 0, 4
# Points of the exact line in each ply.
SAMPLES = 24


def column(step, x, y, k):
    """(z, component k of S) at the nodes of the line (x, y) through the thickness."""
    return sorted((p[2], s[k]) for p, s in zip(step.points, step.point["S"])
                  if abs(p[0] - x) < 1e-3 and abs(p[1] - y) < 1e-3)


def exact(plate, x, y, k):
    """(z, component k of the exact stress) in each ply from the bottom up. At an interface, the
    line has the value of the lower ply, then the value of the upper ply."""
    h = THICK["h"]
    rows = []
    for ply in range(3):
        z0 = -h / 2 + ply * h / 3
        for i in range(SAMPLES + 1):
            z = z0 + h / 3 * i / SAMPLES
            # pagano.py gives the lower ply at an interface: move the first point into this ply.
            rows.append((z, plate.stress(x, y, z + 1e-9 if i == 0 and ply else z)[k]))
    return rows


def write(path, head, model, line, error):
    """error: the largest difference of the model from Pagano, (z, difference)."""
    rows = [f"{z:.4f},{v:.4f}," for z, v in model] + [f"{z:.4f},,{v:.4f}" for z, v in line]
    note = f"# note: The largest difference of the model from Pagano is {error[1]:+.3f} MPa, at z = {error[0]:.2f} mm.\n"
    path.write_text(head + note + "z [mm],model,Pagano\n" + "\n".join(rows) + "\n")


def largest(model, plate, x, y, k):
    return max(((z, v - plate.stress(x, y, z)[k]) for z, v in model), key=lambda e: abs(e[1]))


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    step = vtu.read(Path(runs["job.inp"]) / "step-1.vtu")
    plate = Exact(THICK["h"], THICK["q0"])
    h, x0 = THICK["h"], THICK["x0"]
    # The nodes on the 2 ply interfaces have the mean stress of the 2 plies: leave them out.
    interface = lambda z: min(abs(z + h / 6), abs(z - h / 6)) < 1e-3
    sx = [(z, v) for z, v in column(step, x0 + A / 2, A / 2, S11) if not interface(z)]
    write(Path(out) / "sx.csv",
          "# title: The thick plate (a/h = 4): σ_xx through the thickness at the centre\n"
          "# y: σ_xx [MPa]\n"
          "# note: Pagano is the exact 3D solution of Pagano (1970) from pagano.py. The plies are 0, 90 and 0 degrees from the bottom.\n"
          "# note: The model line is the nodes of the line x = a/2, y = a/2. It leaves out the 2 nodes on the ply interfaces "
          f"z = {-h / 6:.2f} and {h / 6:.2f} mm. There the result file of ccx gives the mean of the 2 plies.\n",
          sx, exact(plate, A / 2, A / 2, SXX), largest(sx, plate, A / 2, A / 2, SXX))
    txz = column(step, x0, A / 2, S13)
    write(Path(out) / "txz.csv",
          "# title: The thick plate (a/h = 4): τ_xz through the thickness at the edge x = 0, y = a/2\n"
          "# y: τ_xz [MPa]\n"
          "# note: Pagano is the exact 3D solution of Pagano (1970) from pagano.py. τ_xz is continuous at the ply interfaces.\n",
          txz, exact(plate, 0.0, A / 2, TXZ), largest(txz, plate, 0.0, A / 2, TXZ))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
