"""The curve of the report of hyperelastic-tube (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/pressure.csv: the
pressure on the inner face against the inner stretch lambda_a at each increment, with the exact
pressure of reference.py. Only the Python standard library.
"""

import contextlib
import io
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE.parents[1]), str(HERE)]
import vtu  # noqa: E402

with contextlib.redirect_stdout(io.StringIO()):
    # reference.py prints the reference values when it loads.
    from reference import A, T, pressure  # noqa: E402


def forces(dat):
    """{time: the sum of the x reactions of the set} for each force block of job.dat."""
    out = {}
    for block in re.split(r"\n (?=forces|displacements)", dat)[1:]:
        m = re.match(r"forces \(fx,fy,fz\) for set \w+ and time\s+(\S+)", block)
        if m:
            rows = re.findall(r"^\s+\d+\s+(\S+)\s+\S+\s+\S+$", block, re.M)
            out[float(m[1])] = sum(float(fx) for fx in rows)
    return out


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    folder = Path(runs["job.inp"])
    force = forces((folder / "ccx/job.dat").read_text())
    rows = []
    for time, name in re.findall(r'timestep="([^"]*)"[^>]*file="([^"]*)"', (folder / "results.pvd").read_text()):
        step = vtu.read(folder / name)
        # The inner node on y = 0: its x displacement is the radial displacement of the inner face.
        u = next(u[0] for p, u in zip(step.points, step.point["U"]) if abs(p[0] - A) < 1e-6 and abs(p[1]) < 1e-6)
        la = 1 + u / A
        # The force on the inner face of the quarter in x is F = p a t, with a = lambda_a A.
        rows.append((la, force[float(time)] / (la * A * T), pressure(la)))
    worst = max(rows, key=lambda r: abs(r[1] / r[2] - 1))
    (Path(out) / "pressure.csv").write_text(
        "# title: The pressure on the inner face against the inner stretch\n"
        "# y: p [MPa]\n"
        "# note: exact is the pressure of the incompressible tube of Haughton and Ogden (1979) from reference.py. "
        "The model p is the sum of the x reactions on the inner face of the quarter, divided by λ_a A t.\n"
        f"# note: The largest difference of the model is {100 * (worst[1] / worst[2] - 1):+.3f} %, at λ_a = {worst[0]:.3f}.\n"
        "λ_a = a / A [1],model,exact\n" + "".join(f"{la:.4f},{p:.6f},{e:.6f}\n" for la, p, e in rows)
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
