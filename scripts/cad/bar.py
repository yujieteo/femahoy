"""build123d recipe: a bar of 100 x 20 x 10 mm, written as a STEP file.

Usage: python bar.py OUT.step [mm|m]
The STEP length unit is mm (the default) or m. The part is the same in both: in m, the STEP
file holds 0.1 x 0.02 x 0.01. `fh info OUT.step --case DIR` must agree with the case `units`.
Install: scripts/install.sh, then run: fh-python bar.py OUT.step
Decision D-17, see DESIGN.md: femahoy ships no CAD code. The agent writes a script like this.
"""
import sys

from build123d import Align, Box, export_step
from OCP.Interface import Interface_Static
from OCP.STEPControl import STEPControl_Controller

LENGTH, WIDTH, HEIGHT = 100.0, 20.0, 10.0  # mm

# The corner is at the origin, so the faces x = 0 and x = LENGTH are the support and the tip.
bar = Box(LENGTH, WIDTH, HEIGHT, align=(Align.MIN, Align.MIN, Align.MIN))
assert abs(bar.volume - LENGTH * WIDTH * HEIGHT) < 1e-6

# build123d models in mm. The STEP writer converts the numbers to its unit and writes the
# unit record. Its default unit is mm. (The `unit` option of export_step names the unit of
# the model, not of the file.)
unit = sys.argv[2] if len(sys.argv) > 2 else "mm"
STEPControl_Controller.Init_s()
if not Interface_Static.SetCVal_s("write.step.unit", {"mm": "MM", "m": "M"}[unit]):
    sys.exit("cannot set the STEP unit")
export_step(bar, sys.argv[1])
