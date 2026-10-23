"""The curve of the report of random-vibration-cantilever (scripts/validation-reports.py).

    python3 curves.py OUT job.inp=FOLDER

FOLDER is the output folder of the run of job.inp. The script writes OUT/psd.csv: the PSD of
the acceleration of the tip against the frequency, from the model (ccx/job.dat) and from the
modal sum of the beam theory in derivation.md, with the input PSD. Only the Python standard
library.
"""

import math
import re
import sys
from pathlib import Path

G = 9806.65  # mm/s2
W = 0.04  # the input PSD, g^2/Hz
F1, F2 = 20.0, 2000.0  # the band, Hz


def model(dat):
    """(f, PSD of A3 in g^2/Hz) at each frequency of the steady state step: the mean over the
    nodes of TIP. ccx prints U at each frequency 2 times, the real part and then the imaginary
    part. The amplitude of the base motion is sqrt(W), thus |U3|^2 is the PSD of U3, and
    (2 pi f)^4 |U3|^2 is the PSD of A3."""
    text = dat.read_text()
    text = text[text.index("S T E P       2") :]
    blocks = re.findall(r"displacements \(vx,vy,vz\) for set TIP and time\s+(\S+)\n\n((?:\s+\d+(?:\s+\S+){3}\n)+)", text)
    out = []
    for (f, real), (f2, imag) in zip(blocks[0::2], blocks[1::2]):
        assert f == f2, (f, f2)
        u3 = [complex(float(a.split()[3]), float(b.split()[3])) for a, b in zip(real.splitlines(), imag.splitlines())]
        f = float(f)
        out.append((f, (2 * math.pi * f) ** 4 * sum(abs(u) ** 2 for u in u3) / len(u3) / G**2))
    return out


def beam():
    """The modes of the beam theory of derivation.md: (omega_r, c_r) for 40 modes."""
    E, rho, b, h, L = 210000.0, 7.85e-9, 10.0, 5.0, 100.0
    I, m = b * h**3 / 12, rho * b * h
    lam = [1.87510407, 4.69409113, 7.85475744, 10.99554073, 14.13716839]
    sig = [0.734095514, 1.018467319, 0.999224497, 1.000033553, 0.999998550]
    lam += [(2 * r - 1) * math.pi / 2 for r in range(6, 41)]
    sig += [1.0] * 35
    return [(l * l / L**2 * math.sqrt(E * I / m), 4 * s / l * (-1) ** r) for r, (l, s) in enumerate(zip(lam, sig))]


def theory(f, modes, zeta=0.02):
    """The PSD of the tip acceleration, |T(f)|^2 W in g^2/Hz (derivation.md, "Sources")."""
    w = 2 * math.pi * f
    t = 1 + w * w * sum(c / (wr * wr - w * w + 2j * zeta * wr * w) for wr, c in modes)
    return abs(t) ** 2 * W


def main(out, folders):
    runs = dict(f.split("=", 1) for f in folders)
    points = model(Path(runs["job.inp"]) / "ccx/job.dat")
    modes = beam()
    # The beam theory on 1000 intervals of log f, merged with the frequencies of the model.
    n = 1000
    grid = [math.exp(math.log(F1) + (math.log(F2) - math.log(F1)) * k / n) for k in range(n + 1)]
    rows = [(f, f"{p:.6g}", "", "") for f, p in points]
    rows += [(f, "", f"{theory(f, modes):.6g}", W) for f in grid]
    rows.sort(key=lambda r: r[0])
    (Path(out) / "psd.csv").write_text(
        "# title: The PSD of the tip acceleration in z\n"
        "# y: PSD of the acceleration [g^2/Hz]\n"
        "# log: x y\n"
        "# points model: this run, the steady state step in ccx/job.dat\n"
        "# note: The model is the mean over the 57 nodes of the tip face, at the 301 frequencies of the "
        "steady state step. The model shows as points, because it lies on the line of the beam theory. "
        "The beam theory is the modal sum of 40 modes in derivation.md. "
        "The RMS of the check is the square root of the area under the curve.\n"
        "frequency [Hz],model,beam theory,base input\n"
        + "\n".join(f"{f:.6g},{a},{b},{c}" for f, a, b, c in rows)
        + "\n"
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
