#!/usr/bin/env bash
# Manual checks on Apple Silicon. Keep the long checks outside CI.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
printf 'Known gap: the CAD mesh does not supply the solver input. The checks use separate .inp fixtures.\n'
printf '%-24s %s\n' check result
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  printf '%-24s %s\n' 'Apple Silicon macOS' FAIL
  exit 1
fi
python="${PYTHON:-$(command -v fh-python || echo python3)}"
gmsh="${FH_GMSH:-gmsh}"
for tool in cargo "$python" "$gmsh"; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf '%-24s FAIL (not found)\n' "$tool"
    exit 1
  fi
done
work="$(mktemp -d "${TMPDIR:-/tmp}/fh-mac-e2e.XXXXXX")"
printf 'Files and logs: %s\n' "$work"
export CARGO_TARGET_DIR="$work/target"
target=aarch64-apple-darwin
run() {
  local name="$1"
  shift
  if "$@" >"$work/$name.log" 2>&1; then
    printf '%-24s PASS\n' "$name"
  else
    printf '%-24s FAIL (see %s)\n' "$name" "$work/$name.log"
    tail -n 10 "$work/$name.log"
    exit 1
  fi
}
run format cargo fmt --check
run clippy cargo clippy --workspace --all-targets --target "$target" -- -D warnings
run tests cargo test --workspace --target "$target"
run release cargo build --release --target "$target" --target-dir "$CARGO_TARGET_DIR" --bin fh --bin fh-solve
fh="$CARGO_TARGET_DIR/$target/release/fh"
export FH_SOLVE="$CARGO_TARGET_DIR/$target/release/fh-solve"
run validation "$fh" validate
run convergence "$fh" validate --convergence validation/cases
run CAD "$python" scripts/cad/bar.py "$work/bar.step"
run STEP-units "$fh" info "$work/bar.step" --case validation/cases/patch-test
run mesh "$fh" mesh "$work/bar.step" -o "$work/bar.msh" --units SI-mm --order 2 --size 5 --gmsh "$gmsh"
run mesh-quality "$fh" info "$work/bar.msh" --case validation/cases/patch-test
run solve-results "$python" - "$FH_SOLVE" "$work" <<'PY'
import pathlib
import subprocess
import sys
import xml.etree.ElementTree as ET

solver, work = sys.argv[1], pathlib.Path(sys.argv[2])
models = [pathlib.Path("fixtures/inp/patch-test.inp"),
          pathlib.Path("fixtures/inp/cantilever.inp"),
          pathlib.Path("validation/cases/lame-cylinder/mesh-4x2.inp")]
for model in models:
    out = work / ("results-" + model.stem)
    # Keep stdin open until the solver exits. EOF means cancel.
    with subprocess.Popen([solver, str(model), str(out), "--formats", "text,vtu"],
                          stdin=subprocess.PIPE) as process:
        status = process.wait()
    if status != 0:
        raise RuntimeError(f"{model}: solver exit {status}")
    if "status: ok" not in (out / "summary.toon").read_text().splitlines():
        raise RuntimeError(f"{model}: result status is not ok")
    datasets = ET.parse(out / "results.pvd").findall(".//DataSet")
    if not datasets:
        raise RuntimeError(f"{model}: no result datasets")
    for dataset in datasets:
        vtu = out / dataset.attrib["file"]
        data = vtu.read_bytes()
        # Raw VTU arrays are binary. Parse only the XML header.
        header, arrays = data.split(b'<AppendedData encoding="raw">', 1)
        piece = ET.fromstring(header + b"</VTKFile>").find(".//Piece")
        if piece is None or any(int(piece.attrib[k]) <= 0 for k in ("NumberOfPoints", "NumberOfCells")):
            raise RuntimeError(f"{vtu}: empty result mesh")
        fields = {a.attrib.get("Name") for a in piece.findall(".//DataArray")}
        if not {"U", "RF", "S", "MISES"} <= fields or not arrays.strip():
            raise RuntimeError(f"{vtu}: result fields are missing")
        if not vtu.with_suffix(".txt").read_text().strip():
            raise RuntimeError(f"{vtu}: text results are empty")
    print(f"{model}: results pass")
PY
