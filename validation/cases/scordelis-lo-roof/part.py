from build123d import Edge, Face, Plane, Vector, export_step

# Scordelis-Lo roof, mid-surface: a cylinder of radius 25000 mm about the x axis.
# The roof spans 40 degrees each side of the crown (z up) and is 50000 mm long in x.
RADIUS, HALF_ANGLE, LENGTH = 25000.0, 40.0, 50000.0

arc = Edge.make_circle(
    RADIUS, Plane(origin=(0, 0, 0), z_dir=(-1, 0, 0), x_dir=(0, 0, 1)), -HALF_ANGLE, HALF_ANGLE
)
export_step(Face.extrude(arc, Vector(LENGTH, 0, 0)), "part.step")
