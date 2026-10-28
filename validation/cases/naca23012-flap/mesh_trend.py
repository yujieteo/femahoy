"""The mesh trend of the report of naca23012-flap (scripts/validation-reports.py), from the mesh
study in derivation.md. It needs no run. Only the Python standard library.
"""

import re
from pathlib import Path


def mesh_report():
    """The plots and compact Markdown tables use the mesh evidence already in derivation.md."""
    text = (Path(__file__).resolve().parent / "derivation.md").read_text()
    mesh, study = text.split("## Mesh study", 1)
    counts = {int(m[1]): int(m[2]) for m in re.finditer(r"^\| ([123])(?: \(the check\))? \| (\d+) \|", mesh, re.M)}
    rows = [[v.strip() for v in line.strip("|").split("|")]
            for line in study.splitlines() if re.match(r"^\| [123] \| 3000 \|", line)]
    panels, curves = [], {}
    for angle, offset in [(-10, 2), (0, 4), (10, 6)]:
        for metric, column in [("cl", offset), ("cm", offset + 1)]:
            name = f"mesh-{angle}-{metric}.csv"
            title = f"{metric} [1], flap {angle} degrees, 3000 iterations"
            values = [(counts[int(row[0])], row[column]) for row in rows]
            curves[name] = (f"# title: Mesh trend: {title}\n# y: {metric} [1]\n"
                            "# note: The 1000-iteration check is a separate series. No extrapolation appears in this plot.\n"
                            f"cells [1],{metric}\n" + "".join(f"{n},{v}\n" for n, v in values))
            trs, previous = [], None
            for n, raw in values:
                v = float(raw)
                change = "First mesh" if previous is None else f"{v - previous:+.5f} ({100 * (v - previous) / abs(previous):+.2f} %)"
                trs.append(f"| {n} | {raw} | {change} |")
                previous = v
            note = "\n\nNo extrapolation: the change grows between the last 2 meshes." if angle == 0 else ""
            panels.append(f"### {title}\n\n<!--curve {name}-->\n\n"
                          f"| Cells | {metric} [1] | Change from previous mesh |\n|---|---|---|\n"
                          + "\n".join(trs) + note)
    return "\n\n".join(panels), curves
