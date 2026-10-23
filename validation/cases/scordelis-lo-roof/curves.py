"""The curve of the report of scordelis-lo-roof (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/mesh.csv: uz at TIP
against the mesh size, with the published value of check.toml. The other mesh sizes come from
the table of derivation.md, "Result". The run of the case mesh (1250 mm) gives its own value
(ccx/job.dat). Only the Python standard library.
"""

import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
SIZE = 1250.0  # the mesh size of part.inp, mm (derivation.md)


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    dat = (Path(runs["job.inp"]) / "ccx/job.dat").read_text()
    uz = float(re.search(r"for set TIP and time\s+\S+\n\n\s+\d+\s+\S+\s+\S+\s+(\S+)", dat)[1])
    study = {float(h): float(u) for h, u in re.findall(r"^\| (\d+) mm \| \d+ \| (\S+) mm \|", (HERE / "derivation.md").read_text(), re.M)}
    study[SIZE] = uz
    reference = tomllib.loads((HERE / "check.toml").read_text())["tip_deflection_z"]["value"]
    (Path(out) / "mesh.csv").write_text(
        "# title: The deflection of the middle of the free edge against the mesh size\n"
        "# y: uz at TIP [mm]\n"
        "# log: x\n"
        "# points MacNeal and Harder: MacNeal and Harder (1985), the Scordelis-Lo roof, 0.3024 in the units of the paper\n"
        f"# note: S6 shells. The value at {SIZE:g} mm, the mesh of the case, comes from this run. "
        "The other mesh sizes come from the table of derivation.md, \"Result\".\n"
        "mesh size [mm],model,MacNeal and Harder\n"
        + "".join(f"{h:g},{u:g},{reference}\n" for h, u in sorted(study.items()))
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
