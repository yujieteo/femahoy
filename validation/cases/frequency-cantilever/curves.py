"""The curve of the report of frequency-cantilever (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/modes.csv: the frequency
of each bending mode of the run (ccx/job.dat) divided by the frequency of the beam theory. The
effective modal mass in y and z of job.dat gives the direction of each mode. Only the Python
standard library.
"""

import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
# Blevins (1979), Table 8-1, the clamped-free beam: lambda_r of the first 3 modes.
LAM = [1.87510407, 4.69409113, 7.85475744]
DEPTH = {"z": 5.0, "y": 10.0}  # the beam is 10 mm in y and 5 mm in z (derivation.md)


def table(text, title):
    """{mode: [the numbers of its row]} of the table under the title in job.dat."""
    text = text[text.index(title) :]
    rows = re.findall(r"^\s+(\d+)((?:\s+-?\d\.\d+E[+-]\d+)+)\s*$", text, re.M)
    out = {}
    for mode, numbers in rows:
        if int(mode) in out or (out and int(mode) != max(out) + 1):
            break
        out[int(mode)] = [float(v) for v in numbers.split()]
    return out


def listed(modes):
    """1, 3 and 6"""
    return ", ".join(str(m) for m in modes[:-1]) + f" and {modes[-1]}" if len(modes) > 1 else str(modes[0])


def main(out, folders):
    f1 = tomllib.loads((HERE / "check.toml").read_text())["f1"]["value"]
    dat = (Path(dict(f.split("=", 1) for f in folders)["job.inp"]) / "ccx/job.dat").read_text()
    freq = {m: row[2] for m, row in table(dat, "E I G E N V A L U E   O U T P U T").items()}
    mass = table(dat, "E F F E C T I V E   M O D A L   M A S S")
    total = float(re.search(r"T O T A L   E F F E C T I V E   M A S S.*?\n\s+(\d\.\d+E[+-]\d+)", dat, re.S)[1])
    # A mode bends the beam in y or z when its effective mass in that direction is 1 % or more of
    # the total mass. Each direction counts its bending modes r = 1, 2, 3.
    bends = {"y": [], "z": []}
    for mode in sorted(freq):
        y, z = mass[mode][1], mass[mode][2]
        if max(y, z) >= 0.01 * total:
            bends["y" if y > z else "z"].append(mode)
    rows = []
    for r in range(len(LAM)):
        ratio = {}
        for d, modes in bends.items():
            if r < len(modes):
                beam = f1 * (LAM[r] / LAM[0]) ** 2 * DEPTH[d] / DEPTH["z"]
                ratio[d] = freq[modes[r]] / beam
        rows.append(f"{r + 1}," + ",".join(f"{ratio[d]:.6f}" if d in ratio else "" for d in "zy") + ",1\n")
    (Path(out) / "modes.csv").write_text(
        "# title: Bending frequencies of the solid against the beam theory\n"
        "# y: f solid / f beam theory [1]\n"
        f"# points bend in z: the solid modes {listed(bends['z'])}, ccx/job.dat of the run\n"
        f"# points bend in y: the solid modes {listed(bends['y'])}, ccx/job.dat of the run\n"
        "# note: The direction of a mode is the direction of its largest effective modal mass in ccx/job.dat.\n"
        f"# note: The beam theory is f_r = f1 (λ_r / λ_1)^2 with f1 = {f1} Hz of check.toml and λ_r of "
        "Blevins (1979), Table 8-1. For a bend in y, the beam is 10 mm deep in place of 5 mm, thus f_r is 2 times larger.\n"
        "bending mode r [1],bend in z,bend in y,beam theory\n" + "".join(rows)
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
