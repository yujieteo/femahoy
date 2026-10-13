#!/usr/bin/env bash
# Run the locked workflow on one case and print a pass or fail table.
# build123d script -> STEP -> fh info -> fh mesh -> fh info -> fh model -> fh-solve -> fh validate.
# The case is validation/cases/bar-tension. Its reference is analytical (E * strain, stress * area).
# Environment: PYTHON (python with build123d), FH_GMSH (gmsh program), KEEP=1 (keep the work folder).
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
python="${PYTHON:-python3}"
gmsh="${FH_GMSH:-gmsh}"
case_name=bar-tension
work="$(mktemp -d "${TMPDIR:-/tmp}/fh-workflow.XXXXXX")"
[[ "${KEEP:-0}" == 1 ]] || trap 'rm -rf "$work"' EXIT
printf 'Work folder: %s\n' "$work"
printf '%-22s %-6s %s\n' step result detail
failed=0
row() { printf '%-22s %-6s %s\n' "$1" "$2" "${3:-}"; }
# step NAME EXPECTED_EXIT COMMAND...: run the command, compare its exit code, keep its output in a log.
step() {
  local name="$1" want="$2"
  shift 2
  "$@" >"$work/$name.log" 2>&1
  local got=$?
  if [[ $got == "$want" ]]; then
    row "$name" PASS "exit $got"
  else
    row "$name" FAIL "exit $got, expected $want (log: $work/$name.log)"
    tail -n 5 "$work/$name.log"
    failed=1
  fi
  return $((got == want ? 0 : 1))
}
finish() {
  if [[ $failed == 0 ]]; then row workflow PASS; exit 0; fi
  row workflow FAIL
  exit 1
}
for tool in cargo "$python" "$gmsh"; do
  command -v "$tool" >/dev/null 2>&1 || { row "$tool" FAIL "not found"; exit 1; }
done
"$python" -c 'import build123d' >/dev/null 2>&1 || { row build123d FAIL "python cannot import build123d"; exit 1; }
step build 0 cargo build --release --bin fh --bin fh-solve || finish
target="${CARGO_TARGET_DIR:-$root/target}/release"
fh="$target/fh"
export FH_SOLVE="$target/fh-solve"
# A scratch repository root that holds only this case.
cdir="$work/root/validation/cases/$case_name"
mkdir -p "$cdir"
cp validation/cases/$case_name/case.toml validation/cases/$case_name/derivation.md "$cdir/"
# Known defect: every fh command loads the case, and the case needs its mesh file to exist.
# A placeholder stands in until fh model writes the real input.
echo '** placeholder' >"$cdir/bar.inp"
step cad-step 0 "$python" scripts/cad/bar.py "$cdir/bar.step" || finish
step step-units 0 "$fh" info "$cdir/bar.step" --case "$cdir" || finish
step mesh 0 "$fh" mesh "$cdir/bar.step" --case "$cdir" --size 8 -o "$cdir/bar.msh" --gmsh "$gmsh" || finish
step mesh-quality 0 "$fh" info "$cdir/bar.msh" --case "$cdir" || finish
step model 0 "$fh" model "$cdir/bar.msh" --case "$cdir" -o "$cdir/bar.inp" || finish
if grep -q placeholder "$cdir/bar.inp"; then row model-output FAIL "placeholder not replaced"; failed=1; finish; fi
step check-inp 0 "$fh" check "$cdir/bar.inp" || finish
# Refusal: the same STEP file with a case in metres must fail with exit 1.
mkdir -p "$work/metre"
sed 's/^units = "SI-mm"/units = "SI-m"/' "$cdir/case.toml" >"$work/metre/case.toml"
cp "$cdir/derivation.md" "$cdir/bar.inp" "$work/metre/"
step refuse-unit-mismatch 1 "$fh" info "$cdir/bar.step" --case "$work/metre"
grep -q 'unit.step.mismatch' "$work/refuse-unit-mismatch.log" || { row refuse-unit-code FAIL "no unit.step.mismatch"; failed=1; }
# Solve and compare with the analytical reference in case.toml.
"$fh" validate --root "$work/root" >"$work/validate.log" 2>&1
code=$?
printf '\n'
cat "$work/validate.log"
printf '\n'
if [[ $code == 0 ]]; then row solve-validate PASS "exit 0"; else row solve-validate FAIL "exit $code"; failed=1; fi
finish
