"""The analytical reference of the case optics-mie-cylinder (derivation.md).

A plane wave in air scatters on an infinite cylinder of radius a and index m, with the electric
field along the axis. The scattered field is a series of cylinder waves with the coefficients
b_n. The scattering efficiency is Q = (2 / x) (|b_0|^2 + 2 sum_{n>=1} |b_n|^2), with the size
parameter x = 2 pi a / wavelength. Run: python3 reference.py (it needs SciPy, as the Python of
Meep has).
"""
import numpy as np
from scipy.special import h1vp, hankel1, jv, jvp

M = 2.0  # index of the cylinder


def coefficients(m, x):
    """The b_n of the scattered wave, n = 0 to x + 4 x^(1/3) + 10. b_n is the same for -n."""
    n = np.arange(int(x + 4 * x ** (1 / 3) + 10) + 1)
    # E_z and its radial derivative are continuous at r = a.
    top = jv(n, m * x) * jvp(n, x) - m * jvp(n, m * x) * jv(n, x)
    bottom = jv(n, m * x) * h1vp(n, x) - m * jvp(n, m * x) * hankel1(n, x)
    return n, top / bottom


def efficiencies(m, x):
    """The scattering and the extinction efficiency. For a real m, the 2 are equal."""
    n, b = coefficients(m, x)
    twice = np.where(n == 0, 1, 2)
    return 2 / x * np.sum(twice * abs(b) ** 2), 2 / x * np.sum(twice * b.real)


if __name__ == "__main__":
    for x in (1.0, 2.0, 2.5, 3.0):
        q_sca, q_ext = efficiencies(M, x)
        print(f"x {x:.1f}: Q {q_sca:.6f}, Q_ext - Q {q_ext - q_sca:.1e}")
