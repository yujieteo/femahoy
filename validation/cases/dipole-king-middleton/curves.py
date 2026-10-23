"""The curves of the report of dipole-king-middleton (scripts/validation-reports.py).

    python3 curves.py OUT omega12.5=FOLDER

FOLDER is the output folder of the run of omega12.5. The script writes OUT/impedance.csv, the
input impedance R + j X of the model (openems/impedance.txt) against beta h near the first
resonance, with the second-order values of King and Middleton and the R of AN-SOF of
derivation.md. Only the Python standard library.
"""

import sys
from pathlib import Path

# beta h, R and X in ohm of King and Middleton (King 1956, Tables 30.6 to 30.10), and R in ohm of
# AN-SOF, for Omega = 12.5, as in the table of derivation.md.
REFERENCE = [
    (1.2, 37.13, -185.7, 36.40),
    (1.3, 46.27, -121.6, 45.82),
    (1.4, 57.39, -60.07, 57.48),
    (1.5, 71.02, 0.27, 72.02),
    (1.6, 87.9, 60.62, 90.37),
    (1.7, 109.2, 122.2, 113.7),
]


def main(out, folders):
    run = Path(dict(f.split("=", 1) for f in folders)["omega12.5"])
    rows = []
    for line in (run / "openems/impedance.txt").read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        _, bh, r, x, _ = map(float, line.split())
        if 1.1 <= bh <= 1.8:
            rows.append(f"{bh:.4f},{r:.3f},{x:.3f},,,\n")
    rows += [f"{bh},,,{r},{x},{r_mom}\n" for bh, r, x, r_mom in REFERENCE]
    (Path(out) / "impedance.csv").write_text(
        "# title: Input impedance of the dipole near the first resonance, Omega = 12.5\n"
        "# y: Z = R + j X [ohm]\n"
        "# points R King-Middleton: R. W. P. King (1956), The Theory of Linear Antennas, Tables 30.6 to 30.10\n"
        "# points X King-Middleton: R. W. P. King (1956), The Theory of Linear Antennas, Tables 30.6 to 30.10\n"
        "# points R AN-SOF: the method-of-moments solution of the same dipole, AN-SOF_vs_King-Middleton_2nd_Order_Impedances.xlsx\n"
        "# note: The model is a strip of width 1.2 mm, 4 cells across its width (openEMS). "
        "beta h = 2 pi h / wavelength, with the half length h = 77.70 mm.\n"
        "# note: The X of AN-SOF is within 1.2 ohm of King-Middleton. The plot does not show it.\n"
        "beta h [1],R model,X model,R King-Middleton,X King-Middleton,R AN-SOF\n" + "".join(rows)
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
