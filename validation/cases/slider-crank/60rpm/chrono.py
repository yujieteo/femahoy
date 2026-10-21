# A slider-crank with its crank at a constant 60 rpm (derivation.md). Units: m, kg, s.
import math

import pychrono as chrono

R, L = 0.1, 0.3  # crank and rod lengths
OMEGA = 2 * math.pi  # crank speed, rad/s
DT, END = 1e-3, 1.125  # time step and end time: 2.25 turns, so the crank ends at 45 degrees
FRAME = 25  # time steps from 1 frame of the page to the next

system = chrono.ChSystemNSC()
system.SetGravitationalAcceleration(chrono.ChVector3d(0, -9.81, 0))
system.SetSolver(chrono.ChSolverSparseQR())
system.SetTimestepperType(chrono.ChTimestepper.Type_HHT)
# With step control, HHT stops at its smallest step on this model. A fixed step converges.
chrono.CastToChTimestepperHHT(system.GetTimestepper()).SetStepControl(False)

ground = chrono.ChBody()
ground.SetFixed(True)
system.Add(ground)


def body(mass, inertia, x, vy, spin):
    """A body with its centre of mass at (x, 0), its inertia about its centre and its velocity."""
    b = chrono.ChBody()
    b.SetMass(mass)
    b.SetInertiaXX(chrono.ChVector3d(inertia, inertia, inertia))
    b.SetPos(chrono.ChVector3d(x, 0, 0))
    b.SetPosDt(chrono.ChVector3d(0, vy, 0))
    b.SetAngVelParent(chrono.ChVector3d(0, 0, spin))
    system.Add(b)
    return b


# The bars are slender: m l^2 / 12 about their centres. The motion is in the x-y plane, so only
# the inertia about z has an effect. The start is the dead centre at x = R + L, with the
# velocities of a steady 60 rpm.
crank = body(0.5, 0.5 * R**2 / 12, R / 2, R * OMEGA / 2, OMEGA)
rod = body(1.5, 1.5 * L**2 / 12, R + L / 2, R * OMEGA / 2, -R * OMEGA / L)
slider = body(2.0, 1e-3, R + L, 0, 0)

motor = chrono.ChLinkMotorRotationAngle()
motor.Initialize(crank, ground, chrono.ChFramed(chrono.ChVector3d(0, 0, 0)))
motor.SetAngleFunction(chrono.ChFunctionRamp(0, OMEGA))
system.Add(motor)
# A planar loop of revolute joints has 3 constraints too many in 3D, and the direct solver fails.
# A spherical joint at the crank pin and a universal joint at the slider remove them. The free
# axes of the universal joint are x and y of its frame. Its z axis is along the rod, so it stops
# the spin of the rod about its axis.
pin = chrono.ChLinkLockSpherical()
pin.Initialize(crank, rod, chrono.ChFramed(chrono.ChVector3d(R, 0, 0)))
system.Add(pin)
along_x = chrono.QuatFromAngleY(math.pi / 2)
wrist = chrono.ChLinkUniversal()
wrist.Initialize(rod, slider, chrono.ChFramed(chrono.ChVector3d(R + L, 0, 0), along_x))
system.Add(wrist)
# The z axis of a prismatic joint is its slide direction.
guide = chrono.ChLinkLockPrismatic()
guide.Initialize(slider, ground, chrono.ChFramed(chrono.ChVector3d(R + L, 0, 0), along_x))
system.Add(guide)

# The page shows each body as a box: length, width and depth in the frame of the body, and the
# z of its centre.
boxes = [(crank, (R, 0.02, 0.01), 0), (rod, (L, 0.015, 0.01), 0.012), (slider, (0.06, 0.04, 0.03), 0.012)]
signs = [(-1, -1, -1), (1, -1, -1), (1, 1, -1), (-1, 1, -1), (-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1)]


def corners(b, size, z):
    """The 8 corners of a box in the order of a hexahedron of gmsh, in the global frame."""
    local = [chrono.ChVector3d(sx * size[0] / 2, sy * size[1] / 2, z + sz * size[2] / 2) for sx, sy, sz in signs]
    return [b.TransformPointLocalToParent(p) for p in local]


start = [corners(*box) for box in boxes]
moves = [[] for _ in boxes]
times = []
rows = []
steps = round(END / DT)
for step in range(steps + 1):
    if step > 0:
        system.DoStepDynamics(DT)
    if step % FRAME == 0:
        times.append(system.GetChTime())
        for k, box in enumerate(boxes):
            moves[k].append([p - p0 for p, p0 in zip(corners(*box), start[k])])
    # GetMotorTorque is the torque of the motor on the crank, about z.
    rows.append((system.GetChTime(), slider.GetPos().x, motor.GetMotorTorque()))

with open("slider-crank.txt", "w") as f:
    f.write("# t x_slider torque\n")
    for row in rows:
        f.write(" ".join(f"{v:.9g}" for v in row) + "\n")
# The motion for the page: the view U holds the move of each corner of each box at each frame.
with open("motion.pos", "w") as f:
    f.write('View "U" {\n')
    for k in range(len(boxes)):
        points = ",".join(f"{p.x:.6g},{p.y:.6g},{p.z:.6g}" for p in start[k])
        values = ",".join(f"{u.x:.6g},{u.y:.6g},{u.z:.6g}" for frame in moves[k] for u in frame)
        f.write(f"VH({points}){{{values}}};\n")
    f.write("TIME{" + ",".join(f"{t:.6g}" for t in times) + "};\n};\n")
