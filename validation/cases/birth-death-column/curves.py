"""The curve of the report of birth-death-column (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/settlement.csv: the
settlement of the top of each lift after step 10 (the set AXIS in ccx/job.dat of the run), with
the lift theory and the column with all lifts at once of derivation.md. Only the Python standard
library.
"""

import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
# derivation.md, "Model": N = 10 lifts of h = 1000 mm, E = 30000 MPa, rho = 2.4e-9 t/mm3, g = 9810 mm/s2.
N, H_LIFT, E, RHO, G = 10, 1000.0, 30000.0, 2.4e-9, 9810.0
C = RHO * G * H_LIFT**2 / (2 * E)


def lifts(i):
    """The settlement of the top of lift i of the column built in lifts, mm."""
    return C * (2 * i * (N + 1 - i) - 1) if i else 0.0


def once(i):
    """The settlement at z = i h of the column with all lifts at once, mm."""
    return C * i * (2 * N - i)


def main(out, folders):
    reference = tomllib.loads((HERE / "check.toml").read_text())["u_mid"]["value"]
    if abs(-lifts(5) / reference - 1) > 1e-6:
        sys.exit(f"curves.py: the lift theory gives {-lifts(5)} at mid-height, check.toml gives {reference}")
    z = {}
    nodes = re.search(r"\*NODE[^\n]*\n(.*?)\n\*", (HERE / "job.inp").read_text(), re.S)[1]
    for line in nodes.splitlines():
        n, *xyz = line.split(",")
        z[int(n)] = float(xyz[2])
    dat = (Path(dict(f.split("=", 1) for f in folders)["job.inp"]) / "ccx/job.dat").read_text()
    blocks = re.findall(r"displacements \(vx,vy,vz\) for set AXIS and time\s+\S+\n\n((?:\s+\d+(?:\s+\S+){3}\n)+)", dat)
    if not blocks:
        sys.exit("curves.py: job.dat has no displacement of AXIS")
    # The last block is after step 10, with all lifts built.
    # 0.0 - u gives 0, not -0, at the base.
    model = {round(z[int(n)] / H_LIFT): 0.0 - float(u) for n, _, _, u in (r.split() for r in blocks[-1].splitlines())}
    if sorted(model) != list(range(N + 1)):
        sys.exit(f"curves.py: AXIS has the lifts {sorted(model)}, not 0 to {N}")
    worst = max(model, key=lambda i: abs(model[i] - lifts(i)))
    (Path(out) / "settlement.csv").write_text(
        "# title: Column built in 10 lifts, the settlement of the top of each lift at the end\n"
        "# y: settlement -u_z [mm]\n"
        "# points ccx: the nodes of the set AXIS after step 10, ccx/job.dat of the run\n"
        "# note: Lift theory: (rho g h^2 / 2E) (2i (N + 1 - i) - 1) at the top of lift i. "
        "All at once: (rho g / E) (H z - z^2 / 2).\n"
        f"# note: The largest difference of ccx from the lift theory is {model[worst] - lifts(worst):+.1e} mm, "
        f"at z = {worst} m. With ADD=WITH STRAIN in place of ADD=STRAIN FREE, ccx gives the curve of all at once.\n"
        "height z [m],lift theory,all at once,ccx\n"
        + "".join(f"{i},{lifts(i):.7g},{once(i):.7g},{model[i]:.7g}\n" for i in range(N + 1))
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
