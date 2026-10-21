"""The closed-form reference of the case slider-crank (derivation.md).

The crank turns at a constant speed w. Thus the power of the motor is the rate of change of the
kinetic and the potential energy: torque * w = dT/dt + dV/dt. Run: python3 reference.py
"""
from math import cos, pi, sin, sqrt

r, l = 0.1, 0.3  # crank and rod lengths, m
m1, m2, m3 = 0.5, 1.5, 2.0  # crank, rod and slider masses, kg
g = 9.81  # gravity along -y, m/s^2
w = 2 * pi  # crank speed, rad/s


def state(theta):
    """The slider position x_B (m) and the motor torque on the crank (N m) at the crank angle."""
    s, c = sin(theta), cos(theta)
    # The rod angle phi: r sin(theta) + l sin(phi) = 0, because the slider is on y = 0.
    sp = -r / l * s
    cp = sqrt(1 - sp * sp)
    phi_d = -r * w * c / (l * cp)
    phi_dd = (r * w * w * s + l * phi_d**2 * sp) / (l * cp)
    xb = r * c + l * cp
    xb_d = -r * w * s - l * phi_d * sp
    xb_dd = -r * w * w * c - l * phi_dd * sp - l * phi_d**2 * cp
    # The velocity and the acceleration of the centre of the rod, A + l/2 (cos phi, sin phi).
    v2 = (-r * w * s - l / 2 * phi_d * sp, r * w * c + l / 2 * phi_d * cp)
    a2 = (
        -r * w * w * c - l / 2 * (phi_dd * sp + phi_d**2 * cp),
        -r * w * w * s + l / 2 * (phi_dd * cp - phi_d**2 * sp),
    )
    # The kinetic energy of the crank is constant.
    i2 = m2 * l * l / 12
    kinetic_d = m2 * (v2[0] * a2[0] + v2[1] * a2[1]) + i2 * phi_d * phi_dd + m3 * xb_d * xb_dd
    potential_d = g * (m1 * r / 2 * w * c + m2 * v2[1])
    return xb, (kinetic_d + potential_d) / w


if __name__ == "__main__":
    for deg in (0, 45, 90, 135, 180):
        xb, torque = state(deg * pi / 180)
        print(f"theta {deg:3d} deg: x_slider {xb:.10f} m, torque {torque:.9f} N m")
