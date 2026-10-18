"""Write 2 touching bodies for the quarter strip. Usage: python warpage_strip.py OUT.step."""
from pathlib import Path
import sys
from build123d import Align, Box, Compound, Pos, export_step

lower = Box(40, 2, 1, align=(Align.MIN, Align.MIN, Align.MIN))
upper = Pos(0, 0, 1) * Box(40, 2, 1, align=(Align.MIN, Align.MIN, Align.MIN))
part = Compound(children=[lower, upper])
assert len(part.solids()) == 2
assert abs(part.volume - 160) < 1e-9
output = Path(sys.argv[1])
export_step(part, output)
output.write_text("\n".join(line.rstrip() for line in output.read_text().splitlines()) + "\n")
