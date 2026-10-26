"""The mesh trend of the report of naca23012-flap (scripts/validation-reports.py), from the mesh
study in derivation.md. It needs no run. Only the Python standard library.
"""

import re
from pathlib import Path


def mesh_report():
    """The plots and compact tables use the mesh evidence already in derivation.md."""
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
                trs.append(f"<tr><td>{n}</td><td>{raw}</td><td>{change}</td></tr>")
                previous = v
            note = "<p>No extrapolation: the change grows between the last 2 meshes.</p>" if angle == 0 else ""
            panels.append(f"<section><h3>{title}</h3><!--curve {name}-->"
                          f"<table><thead><tr><th>Cells</th><th>{metric} [1]</th><th>Change from previous mesh</th></tr></thead>"
                          f"<tbody>{''.join(trs)}</tbody></table>{note}</section>")
    return '<div data-mesh-trend>' + "".join(panels) + '</div>', curves
