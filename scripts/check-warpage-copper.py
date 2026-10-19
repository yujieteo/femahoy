#!/usr/bin/env python3
"""Check the measured warpage of a copper on silicon slice through CAD, fh run, CalculiX and VTU.

Run with a Python that has build123d. Optional argument: a new proof folder.
FH_BIN names fh. FH_CCX names CalculiX. Build the Rust binaries first.
"""
import json
import os
from pathlib import Path
import re
import signal
import struct
import subprocess
import sys
import tempfile
import tomllib
import xml.etree.ElementTree as ET


def arrays(path):
    data = path.read_bytes()
    start = data.index(b"_", data.index(b"<AppendedData")) + 1
    header = ET.fromstring(data[:data.index(b"<AppendedData")] + b"</VTKFile>")
    result = {}
    for item in header.iter("DataArray"):
        offset = start + int(item.attrib["offset"])
        size = struct.unpack_from("<I", data, offset)[0]
        code = {"Float64": "d", "Int64": "q", "Int32": "i", "UInt8": "B"}[item.attrib["type"]]
        count = size // struct.calcsize(code)
        result[item.attrib["Name"]] = struct.unpack_from(f"<{count}{code}", data, offset + 4)
    return result


def plate_warpage(spec, length):
    """Biaxial bilayer curvature (Timoshenko form, plate moduli E/(1-nu)), as the sag over the length."""
    si, cu = spec["model"]["material"]
    m1, m2 = (m["youngs_modulus"] / (1 - m["poissons_ratio"]) for m in (si, cu))
    h1, h2 = si["box"][5] - si["box"][2], cu["box"][5] - cu["box"][2]
    strain = (cu["expansion"] - si["expansion"]) * spec["model"]["temperature_change"]
    curvature = 6 * m1 * m2 * h1 * h2 * (h1 + h2) * strain / (
        m1**2 * h1**4 + 4 * m1 * m2 * h1**3 * h2 + 6 * m1 * m2 * h1**2 * h2**2
        + 4 * m1 * m2 * h1 * h2**3 + m2**2 * h2**4)
    return -curvature * length**2 / 8


def main():
    root = Path(__file__).resolve().parent.parent
    proof = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="fh-warpage-copper-"))
    proof.mkdir(parents=True, exist_ok=True)
    case = proof / "case"
    case.mkdir()
    source = root / "validation/cases/warpage-thick-copper"
    for name in ("case.toml", "derivation.md"):
        (case / name).write_bytes((source / name).read_bytes())
    fh = Path(os.environ.get("FH_BIN", root / "target/debug/fh")).resolve()
    env = dict(os.environ, FH_SOLVE=str(fh.with_name("fh-solve-ccx")), FH_NO_OPEN="1")

    def run(name, args):
        done = subprocess.run(list(map(str, args)), env=env, capture_output=True, text=True)
        log = re.sub(r"token=[^&\s]+", "token=REDACTED", done.stdout + done.stderr)
        (proof / f"{name}.log").write_text(log)
        if done.returncode != 0:
            raise RuntimeError(f"{name}: exit {done.returncode}. See {proof / (name + '.log')}")
        return log

    run("cad", [sys.executable, root / "scripts/cad/thick_copper_slice.py", case / "slice.step"])
    spec = tomllib.loads((case / "case.toml").read_text())
    # Calabretta et al. 2021, Table 2: 0.700 um/K measured from 50 to 90 C on the 50 mm slice.
    measured = -0.700e-3 * spec["model"]["temperature_change"]
    assert abs(measured - spec["ref"]["warpage"]) < 1e-15
    theory = plate_warpage(spec, 50.0)
    rows = []
    for size in (1.0, 0.5, 0.25):
        state = proof / f"state-{size}"
        assert not state.exists(), "use a new proof folder"
        try:
            run(f"run-{size}", [fh, "run", case, "--size", size, "--state", state, "--no-open"])
            outputs = list(state.glob("runs/*/out/step-1.vtu"))
            assert len(outputs) == 1
            out = outputs[0].parent
            vtu = arrays(outputs[0])
            ends = [vtu["U"][3 * i + 2] for i, member in enumerate(vtu["NSET_END"]) if member]
            assert len(ends) == 1
            value = ends[0]
            difference = value - measured
            assert abs(difference) <= spec["rel_tol"] * abs(measured) + spec["abs_tol"]
            # The solve must agree with the plate theory for the same inputs, apart from free-edge effects.
            assert abs(value - theory) <= 0.01 * abs(theory)
            si, cu = vtu["ELSET_SI"], vtu["ELSET_CU"]
            assert all(a + b == 1 for a, b in zip(si, cu))
            conn = vtu["connectivity"]
            nodes = [set(), set()]
            offset = 0
            for i, end in enumerate(vtu["offsets"]):
                nodes[0 if si[i] else 1].update(conn[offset:end])
                offset = end
            shared = nodes[0] & nodes[1]
            assert shared and all(abs(vtu["Points"][3 * n + 2] - 0.73) < 1e-6 for n in shared)
            summary = (out / "summary.toon").read_text()
            wall = float(re.search(r"wall_s: ([0-9.eE+-]+)", summary)[1])
            inp = next((state / "cases").glob("*/*.inp"))
            quality = run(f"quality-{size}", [fh, "info", inp.with_suffix(".msh"), "--case", case])
            jacobian = float(re.search(r"min_scaled_jacobian: ([0-9.eE+-]+)", quality)[1])
            rows.append(dict(mesh_size_mm=size, nodes=len(vtu["NODE_ID"]), elements=len(si), shared_nodes=len(shared),
                             min_scaled_jacobian=jacobian, warpage_mm=value,
                             rate_um_per_k=-value * 1e3 / spec["model"]["temperature_change"],
                             measured_mm=measured, difference_mm=difference, relative_difference=difference / abs(measured),
                             plate_theory_mm=theory, solve_wall_s=wall))
        finally:
            info = state / "daemon.info"
            if info.exists():
                fields = dict(line.split("=", 1) for line in info.read_text().splitlines())
                os.kill(int(fields["pid"]), signal.SIGTERM)
    (proof / "record.json").write_text(json.dumps(rows, indent=2) + "\n")
    for row in rows:
        print(json.dumps(row))
    print(f"PASS: proof in {proof}")


if __name__ == "__main__":
    main()
