#!/usr/bin/env python3
"""Check the public bilayer reference through CAD, fh run, CalculiX and VTU.

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


def main():
    root = Path(__file__).resolve().parent.parent
    proof = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="fh-warpage-"))
    proof.mkdir(parents=True, exist_ok=True)
    case = proof / "case"
    case.mkdir()
    source = root / "validation/cases/warpage-strip"
    for name in ("case.toml", "derivation.md"):
        (case / name).write_bytes((source / name).read_bytes())
    fh = Path(os.environ.get("FH_BIN", root / "target/debug/fh")).resolve()
    env = dict(os.environ, FH_SOLVE=str(fh.with_name("fh-solve-ccx")), FH_NO_OPEN="1")

    def run(name, args, expected=0, custom_env=env):
        done = subprocess.run(list(map(str, args)), env=custom_env, capture_output=True, text=True)
        log = re.sub(r"token=[^&\s]+", "token=REDACTED", done.stdout + done.stderr)
        (proof / f"{name}.log").write_text(log)
        if done.returncode != expected:
            raise RuntimeError(f"{name}: exit {done.returncode}, expected {expected}. See {proof / (name + '.log')}")
        return log

    run("cad", [sys.executable, root / "scripts/cad/warpage_strip.py", case / "strip.step"])
    spec = tomllib.loads((case / "case.toml").read_text())
    # Lubarda and Lubarda, equations 2.5 to 2.7, with the selected inputs.
    c1, c2 = 200000 * 4, 70000 * 4
    stiffness = (c1 + c2) / 12
    curvature = 2 * c1 * c2 * 2 * (2.3e-5 - 1.2e-5) * 100 / (4 * (c1 + c2) * stiffness + c1 * c2 * 4)
    reference = -curvature * 40**2 / 2
    assert abs(reference - spec["ref"]["warpage"]) < 1e-14
    rows = []
    for size in (1.0, 0.5):
        state = proof / f"state-{size}"
        assert not state.exists(), "use a new proof folder"
        try:
            run(f"run-{size}", [fh, "run", case, "--size", size, "--state", state, "--no-open"])
            outputs = list(state.glob("runs/*/out/step-1.vtu"))
            assert len(outputs) == 1
            out = outputs[0].parent
            vtu = arrays(outputs[0])
            tips = [vtu["U"][3 * i + 2] for i, member in enumerate(vtu["NSET_TIP"]) if member]
            value = sum(tips) / len(tips)
            difference = abs(value - reference)
            assert difference <= spec["rel_tol"] * abs(reference) + spec["abs_tol"]
            low, high = vtu["ELSET_LOW"], vtu["ELSET_HIGH"]
            assert all(a + b == 1 for a, b in zip(low, high))
            conn = vtu["connectivity"]
            nodes = [set(), set()]
            offset = 0
            for i, end in enumerate(vtu["offsets"]):
                nodes[0 if low[i] else 1].update(conn[offset:end])
                offset = end
            shared = nodes[0] & nodes[1]
            assert shared and all(abs(vtu["Points"][3 * n + 2] - 1) < 1e-6 for n in shared)
            summary = (out / "summary.toon").read_text()
            wall = float(re.search(r"wall_s: ([0-9.eE+-]+)", summary)[1])
            inp = next((state / "cases").glob("*/*.inp"))
            run(f"check-{size}", [fh, "check", inp])
            run(f"quality-{size}", [fh, "info", inp.with_suffix(".msh"), "--case", case])
            rows.append(dict(mesh_size_mm=size, nodes=len(vtu["NODE_ID"]), elements=len(low), shared_nodes=len(shared),
                             warpage_mm=value, reference_mm=reference, difference_mm=difference,
                             relative_difference=difference / abs(reference), solve_wall_s=wall))
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
