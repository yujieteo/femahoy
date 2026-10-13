"""build123d recipe: a bar of 100 x 20 x 10, written as a STEP file in mm.

Usage: python bar.py OUT.step
Install: uv venv -p 3.12 venv && uv pip install -p venv/bin/python build123d
Decision D-11, see DESIGN.md: femahoy ships no CAD code. The agent writes a script like this.
The numbers below are in mm. `fh info OUT.step --case DIR` must agree with the case `units`.
"""
import sys

from build123d import Box, Align, export_step

LENGTH, WIDTH, HEIGHT = 100.0, 20.0, 10.0  # mm

# The corner is at the origin, so the faces x = 0 and x = LENGTH are the fixed face and the tip.
bar = Box(LENGTH, WIDTH, HEIGHT, align=(Align.MIN, Align.MIN, Align.MIN))
assert abs(bar.volume - LENGTH * WIDTH * HEIGHT) < 1e-6

# build123d writes a millimetre record. A STEP file with another unit is refused by `fh info`.
export_step(bar, sys.argv[1])
