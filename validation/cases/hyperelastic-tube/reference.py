"""Print the reference values of check.toml: the exact inflation of an incompressible tube.

Usage: python3 reference.py

Haughton and Ogden (1979b), with the axial stretch 1 (plane strain):
  p = integral from lb to la of w'(l) / (l^2 - 1) dl,  w(l) = W(l, 1/l, 1),
  lb^2 - 1 = (A/B)^2 (la^2 - 1).
W is the Ogden energy sum mu_p / alpha_p (l1^alpha_p + l2^alpha_p + l3^alpha_p - 3) with the
constants of Ogden (1972) for the data of Treloar (1944).
"""

import math

A, B, T = 10.0, 20.0, 1.0  # mm
KG = 0.0980665  # MPa in 1 kg/cm2
MU = [6.3 * KG, 0.012 * KG, -0.1 * KG]
ALPHA = [1.3, 5.0, -2.0]


def dw(l):
    return sum(m * (l ** (a - 1) - l ** (-a - 1)) for m, a in zip(MU, ALPHA))


def outer(la):
    return math.sqrt(1 + (A / B) ** 2 * (la * la - 1))


def pressure(la, steps=4000):
    """Simpson's rule. The integrand is smooth: at l = 1 it has the finite limit
    sum mu_p alpha_p = 2 mu0."""
    lb = outer(la)
    h = (la - lb) / steps
    f = [dw(lb + k * h) / ((lb + k * h) ** 2 - 1) for k in range(steps + 1)]
    return h / 3 * (f[0] + f[-1] + 4 * sum(f[1:-1:2]) + 2 * sum(f[2:-1:2]))


print(f"mu0 = {0.5 * sum(m * a for m, a in zip(MU, ALPHA)):.6f} MPa")
for la in (2.5, 4.0):
    p = pressure(la)
    print(f"la = {la}: p = {p:.6f} MPa, F = p la A T = {p * la * A * T:.6f} N")
print(f"la = 4.0: lb = {outer(4.0):.8f}, u(B) = {B * (outer(4.0) - 1):.6f} mm")
# The maximum of p, by golden section search on 2 < la < 3.
lo, hi, g = 2.0, 3.0, (math.sqrt(5) - 1) / 2
for _ in range(60):
    c, d = hi - g * (hi - lo), lo + g * (hi - lo)
    lo, hi = (lo, d) if pressure(c) > pressure(d) else (c, hi)
print(f"maximum: la = {lo:.4f}, p = {pressure(lo):.6f} MPa")
