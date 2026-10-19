"""Write the quarter model of the 2.5D package with a metal frame of Tsai et al. 2021 (Fig. 1c, Fig. 3b).

Usage: python package_2p5d.py OUT.step. The derivation of the case warpage-2p5d-frame gives each size
that the paper does not give. `fh mesh --volume` selects a body when the body is fully in a box. Thus
each L-shaped part of the quarter (EMC, fillet, adhesive, frame) is cut into pieces whose boxes hold
no other body. The script prints the name, the volume and the box of each body.
"""
from pathlib import Path
import sys
from build123d import Align, Box, Compound, Plane, Polyline, Pos, extrude, make_face, export_step

H = 27.5                    # half of the 55 mm package
SUB_T = 1.81                # substrate
UF2_T, INT_T, UF1_T, DIE_T = 0.10, 0.09, 0.04, 0.69
MX, MY = 20.5, 13.75        # half of the 41 x 27.5 mm molded module
DX, DY = 19.5, 12.2         # half of the 39.0 x 24.4 mm die block
FIL = 0.9                   # width of the UF2 fillet on the substrate
ADH_T, FRM_T = 0.06, 0.875
FX, FY = H - 3.2, H - 7.4   # inner edges of the frame

z_uf2, z_int, z_uf1, z_die = SUB_T, SUB_T + UF2_T, SUB_T + UF2_T + INT_T, SUB_T + UF2_T + INT_T + UF1_T
z_top = z_die + DIE_T
low = (Align.MIN, Align.MIN, Align.MIN)


def box(x0, y0, z0, x1, y1, z1):
    return Pos(x0, y0, z0) * Box(x1 - x0, y1 - y0, z1 - z0, align=low)


def wedge_x():
    """Fillet on the x = MX side: a triangle in the x-z plane, extruded over y = 0..MY."""
    tri = Polyline((MX, z_uf2), (MX + FIL, z_uf2), (MX, z_top), close=True)
    return extrude(make_face(Plane.XZ * tri), amount=-MY)


def wedge_y():
    """Fillet on the y = MY side: a triangle in the y-z plane, extruded over x = 0..MX."""
    tri = Polyline((MY, z_uf2), (MY + FIL, z_uf2), (MY, z_top), close=True)
    return extrude(make_face(Plane.YZ * tri), amount=MX)


def fillet_corner():
    """The mitred corner: the volume under both fillet planes over the corner square."""
    long_x = Polyline((MX, z_uf2), (MX + FIL, z_uf2), (MX, z_top), close=True)
    long_y = Polyline((MY, z_uf2), (MY + FIL, z_uf2), (MY, z_top), close=True)
    a = Pos(0, MY + FIL, 0) * extrude(make_face(Plane.XZ * long_x), amount=FIL)
    b = Pos(MX, 0, 0) * extrude(make_face(Plane.YZ * long_y), amount=FIL)
    return a & b


bodies = {
    "SUB": box(0, 0, 0, H, H, SUB_T),
    "UF2": box(0, 0, z_uf2, MX, MY, z_int),
    "INT": box(0, 0, z_int, MX, MY, z_uf1),
    "UF1": box(0, 0, z_uf1, DX, DY, z_die),
    "DIE": box(0, 0, z_die, DX, DY, z_top),
    "EMC_X": box(DX, 0, z_uf1, MX, MY, z_top),
    "EMC_Y": box(0, DY, z_uf1, DX, MY, z_top),
    "FIL_X": wedge_x(),
    "FIL_Y": wedge_y(),
    "FIL_C": fillet_corner(),
    "ADH_X": box(FX, 0, SUB_T, H, H, SUB_T + ADH_T),
    "ADH_Y": box(0, FY, SUB_T, FX, H, SUB_T + ADH_T),
    "FRM_X": box(FX, 0, SUB_T + ADH_T, H, H, SUB_T + ADH_T + FRM_T),
    "FRM_Y": box(0, FY, SUB_T + ADH_T, FX, H, SUB_T + ADH_T + FRM_T),
}
for name, body in bodies.items():
    bb = body.bounding_box()
    print(f"{name}\t{body.volume:.6f}\tbox:{bb.min.X:.4f},{bb.min.Y:.4f},{bb.min.Z:.4f},{bb.max.X:.4f},{bb.max.Y:.4f},{bb.max.Z:.4f}")
fil_volume = 0.5 * FIL * (z_top - z_uf2)
assert abs(bodies["FIL_X"].volume - fil_volume * MY) < 1e-9
assert abs(bodies["FIL_Y"].volume - fil_volume * MX) < 1e-9
assert abs(bodies["FIL_C"].volume - FIL * FIL * (z_top - z_uf2) / 3) < 1e-9
part = Compound(children=list(bodies.values()))
assert len(part.solids()) == len(bodies)
output = Path(sys.argv[1])
export_step(part, output)
output.write_text("\n".join(line.rstrip() for line in output.read_text().splitlines()) + "\n")
