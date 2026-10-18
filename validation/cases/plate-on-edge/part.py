# Steel plate 3756 x 1000 x 2.4 mm on its edge, origin at the middle.
# x = length, y = height (up), z = thickness.
# The top edge (y = +500) has pads of 2 x 50 mm at the load points x = +-1200, +-1500.
# The bottom edge (y = -500) has pads of 20 mm at the pins x = +-1300.
# The part is one prism along z, so the pads are faces of the side walls.
from build123d import Face, Solid, Vector, Wire, export_step

L2, H2, T = 1878.0, 500.0, 2.4
loads = [-1500.0, -1200.0, 1200.0, 1500.0]
pins = [-1300.0, 1300.0]
half_pad, half_pin = 50.0, 10.0

bottom = sorted({-L2, L2, *(p + d for p in pins for d in (-half_pin, half_pin))})
top = sorted({-L2, L2, *(x + d for x in loads for d in (-half_pad, 0.0, half_pad))}, reverse=True)

z0 = -T / 2
pts = [Vector(x, -H2, z0) for x in bottom] + [Vector(x, H2, z0) for x in top]
face = Face(Wire.make_polygon(pts, close=True))
plate = Solid.extrude(face, Vector(0, 0, T))

bb = plate.bounding_box()
print(f"faces {len(plate.faces())}, volume {plate.volume:.1f} mm3")
print(f"bbox {bb.min.X:.1f} {bb.min.Y:.1f} {bb.min.Z:.2f} .. {bb.max.X:.1f} {bb.max.Y:.1f} {bb.max.Z:.2f}")
export_step(plate, "part.step")
