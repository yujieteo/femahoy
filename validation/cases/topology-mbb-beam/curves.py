"""The curves of the report of topology-mbb-beam (scripts/validation-reports.py).

    python3 curves.py OUT 60x20=FOLDER

FOLDER is the output folder of the run of 60x20. The script writes OUT/history.csv, the
compliance of each iteration of the loop (topology/history.txt), and OUT/history-end.csv, the
last iterations with the pass band of check.toml. The reference is c of Andreassen et al.
(2011) in check.toml. Only the Python standard library.
"""

import sys
import tomllib
from pathlib import Path

CHECK = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
REFERENCE = CHECK["compliance"]["value"]
REL_TOL = CHECK["rel_tol"]
SOURCE = "Andreassen et al. (2011), figure 3, top left, the final design"
END = 80  # the first iteration of the second plot


def main(out, folders):
    out = Path(out)
    runs = dict(f.split("=", 1) for f in folders)
    table = Path(runs["60x20"]) / "topology/history.txt"
    rows = [line.split() for line in table.read_text().splitlines() if not line.startswith("#")]
    history = [(int(i), float(c)) for i, c, _, _ in rows]
    last = history[-1][0]
    lines = [f"{i},{c:.4f}," for i, c in history] + [f"{last},,{REFERENCE}"]
    (out / "history.csv").write_text(
        "# title: The compliance of the design at each iteration\n"
        "# y: compliance c [1]\n"
        f"# points paper: {SOURCE}\n"
        "# note: Iteration 1 is the uniform design with the density 0.5. The paper gives only the final "
        "compliance: the plot shows it at the last iteration of the model.\n"
        "iteration [1],model,paper\n" + "\n".join(lines) + "\n"
    )
    low, high = REFERENCE * (1 - REL_TOL), REFERENCE * (1 + REL_TOL)
    lines = [f"{i},{c:.4f},,{low:.4f},{high:.4f}" for i, c in history if i >= END]
    lines.append(f"{last},,{REFERENCE},,")
    (out / "history-end.csv").write_text(
        "# title: The compliance of the last iterations, with the pass band of the check\n"
        "# y: compliance c [1]\n"
        f"# points paper: {SOURCE}\n"
        f"# note: The pass band is {REFERENCE} +/- {REL_TOL * 100:g} % (check.toml). The loop stops at the first iteration "
        "where no density changes by more than 0.01.\n"
        "iteration [1],model,paper,band low,band high\n" + "\n".join(lines) + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
