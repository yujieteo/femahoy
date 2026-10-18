"""Write the quarter wafer slice: 730 um silicon and 20 um copper. Usage: python thick_copper_slice.py OUT.step."""
from pathlib import Path
import sys
from build123d import Align, Box, Compound, Pos, export_step

low = (Align.MIN, Align.MIN, Align.MIN)
silicon = Box(25, 5, 0.73, align=low)
copper = Pos(0, 0, 0.73) * Box(25, 5, 0.02, align=low)
part = Compound(children=[silicon, copper])
assert len(part.solids()) == 2
assert abs(part.volume - 25 * 5 * 0.75) < 1e-9
output = Path(sys.argv[1])
export_step(part, output)
output.write_text("\n".join(line.rstrip() for line in output.read_text().splitlines()) + "\n")
