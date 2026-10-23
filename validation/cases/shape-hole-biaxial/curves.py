"""The curves of the report of shape-hole-biaxial (scripts/validation-reports.py).

    python3 curves.py OUT L40=FOLDER

FOLDER is the output folder of the run of L40. The script writes 2 curves:

- OUT/loop.csv: the peak stress, the ratio and the power of each solve of the loop
  (optimize/history.txt), with the exact optimum.
- OUT/edge.csv: the von Mises stress on the edge of the best hole (the node set HOLE of
  optimize/page.inp) against the angle from the x axis, with the exact hoop stress sx + sy.

Only the Python standard library.
"""

import math
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import vtu  # noqa: E402

# derivation.md: the ellipse with a/b = sx/sy = 2 (power 2) has the hoop stress sx + sy = 1.5 at
# each point of its edge. No hole gives a lower peak. check.toml has the 3 values.
CHECK = tomllib.loads((Path(__file__).resolve().parent / "check.toml").read_text())
RATIO, POWER, PEAK = (CHECK[k]["value"] for k in ("ratio", "power", "objective"))


def hole(inp):
    """The (x, y) of each node of the node set HOLE of a ccx input."""
    nodes, ids, mode = {}, [], None
    for line in inp.read_text().splitlines():
        if line.startswith("*"):
            key = line.upper().replace(" ", "")
            mode = "node" if key.split(",")[0] == "*NODE" else "hole" if key.startswith("*NSET,NSET=HOLE") else None
        elif mode == "node":
            f = line.split(",")
            nodes[int(f[0])] = (float(f[1]), float(f[2]))
        elif mode == "hole":
            ids += [int(n) for n in line.split(",") if n.strip()]
    return [nodes[n] for n in ids]


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    folder = Path(runs["L40"])
    history = [line.split() for line in (folder / "optimize/history.txt").read_text().splitlines() if not line.startswith("#")]
    (Path(out) / "loop.csv").write_text(
        "# title: The loop: each solve of the hole shape\n"
        "# y: peak, ratio and power [1]\n"
        "# note: peak is the largest von Mises stress on the hole edge. The exact optimum is the ellipse with ratio 2 and power 2, "
        "and its peak 1.5 is the lowest peak of all hole shapes (derivation.md).\n"
        "solve [1],peak,ratio,power,exact peak,exact ratio and power\n"
        + "".join(f"{n},{float(peak):.5f},{float(ratio):.5f},{float(power):.5f},{PEAK},{RATIO}\n"
                  for n, ratio, power, peak in history)
    )
    step = vtu.read(folder / "step-1.vtu")
    near = [(p, m) for p, m in zip(step.points, step.point["MISES"]) if p[0] ** 2 + p[1] ** 2 < 9]
    rows = []
    for x, y in hole(folder / "optimize/page.inp"):
        # The step file has each node of the plane model 2 or 3 times across the thickness.
        values = [m for p, m in near if abs(p[0] - x) < 1e-5 and abs(p[1] - y) < 1e-5]
        rows.append((math.degrees(math.atan2(y, x)), sum(values) / len(values)))
    rows.sort()
    low, high = min(m for _, m in rows), max(m for _, m in rows)
    (Path(out) / "edge.csv").write_text(
        "# title: The stress on the edge of the best hole\n"
        "# y: von Mises stress [1]\n"
        f"# note: On the free edge, the von Mises stress is equal to the absolute value of the hoop stress. The model is between {low:.4f} and {high:.4f} "
        "at the nodes of the edge. The exact ellipse has the hoop stress sx + sy = 1.5 at each point of its edge (derivation.md).\n"
        "angle from the x axis [degrees],model,exact ellipse\n"
        + "".join(f"{a:.3f},{m:.5f},{PEAK}\n" for a, m in rows)
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
