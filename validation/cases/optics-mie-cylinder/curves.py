"""The curves of the report of optics-mie-cylinder (scripts/validation-reports.py).

    python3 curves.py OUT res40=FOLDER

FOLDER is the output folder of the run of res40. The script writes OUT/spectrum.csv, the
scattering efficiency Q of the run (meep/spectrum.txt) and of the exact series against the size
parameter, and OUT/error.csv, the difference of the 2 in %. spectrum-exact.txt in the case folder
is the exact Q at the same 41 size parameters (reference.py --spectrum). Only the Python standard
library.
"""

import sys
from pathlib import Path


def table(path):
    """The rows (x, Q) of a file with the columns x and Q and # comment lines."""
    rows = [line.split() for line in path.read_text().splitlines() if line.strip() and not line.startswith("#")]
    return [(float(x), float(q)) for x, q in rows]


def main(out, folders):
    out = Path(out)
    run = Path(dict(f.split("=", 1) for f in folders)["res40"])
    model = table(run / "meep/spectrum.txt")
    exact = table(Path(__file__).resolve().parent / "spectrum-exact.txt")
    if [round(x, 4) for x, _ in model] != [round(x, 4) for x, _ in exact]:
        sys.exit("curves.py: spectrum.txt and spectrum-exact.txt have different size parameters")
    (out / "spectrum.csv").write_text(
        "# title: Scattering efficiency of the cylinder against the size parameter\n"
        "# y: Q = C / (2 a) [1]\n"
        "# points exact series: Bohren and Huffman (1983), chapter 8, the series solution (reference.py)\n"
        "# note: Meep: 1 pulse, 40 grid points per a. The dots are the exact Q at the same size "
        "parameters, so the Meep line shows between them.\n"
        "x = 2 pi a / wavelength [1],Meep,exact series\n"
        + "".join(f"{x:.4f},{q:.6f},{e:.6f}\n" for (x, q), (_, e) in zip(model, exact))
    )
    (out / "error.csv").write_text(
        "# title: Difference of the Meep Q from the exact Q\n"
        "# y: (Q Meep - Q exact) / Q exact [%]\n"
        "# note: rel_tol of check.toml is 1 %. It applies to the 4 checks at x = 1, 2, 2.5 and 3.\n"
        "x = 2 pi a / wavelength [1],Meep,+rel_tol,-rel_tol\n"
        + "".join(f"{x:.4f},{100 * (q - e) / e:.4f},1,-1\n" for (x, q), (_, e) in zip(model, exact))
    )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
