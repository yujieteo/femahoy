"""The curve of the report of scordelis-lo-roof (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/mesh.csv: uz at TIP
against the number of elements N along each side of the N x N mesh, with the published value
of check.toml. The other meshes come from the table of derivation.md, "Mesh convergence". The run
of the case mesh (24 x 24) gives its own value (ccx/job.dat). Only the Python standard library.
"""

import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
N = 24  # the elements along each side of part.inp (derivation.md)


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    dat = (Path(runs["job.inp"]) / "ccx/job.dat").read_text()
    uz = float(re.search(r"for set TIP and time\s+\S+\n\n\s+\d+\s+\S+\s+\S+\s+(\S+)", dat)[1])
    study = {int(n): float(u) for n, u in re.findall(r"^\| (\d+) x \d+[^|]* \| \d+ \| \d+ \| (\S+) \|", (HERE / "derivation.md").read_text(), re.M)}
    study[N] = uz
    reference = tomllib.loads((HERE / "check.toml").read_text())["tip_deflection_z"]["value"]
    (Path(out) / "mesh.csv").write_text(
        "# title: The deflection of the middle of the free edge against the mesh\n"
        "# y: uz at TIP [mm]\n"
        "# log: x\n"
        "# points MacNeal and Harder: MacNeal and Harder (1985), the Scordelis-Lo roof, 0.3024 in the units of the paper\n"
        f"# note: S8R shells. The value at N = {N}, the mesh of the case, comes from this run. "
        "The other meshes come from the table of derivation.md, \"Mesh convergence\".\n"
        "elements along each side N [1],model,MacNeal and Harder\n"
        + "".join(f"{n},{u:g},{reference}\n" for n, u in sorted(study.items()))
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
