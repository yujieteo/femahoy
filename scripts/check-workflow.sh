#!/usr/bin/env bash
# Run the locked workflow and print a pass or fail table.
# build123d script -> STEP -> fh info -> fh mesh -> fh info -> fh model -> fh-solve -> fh validate.
# Cases: bar-tension (end displacement, plane sets), and bar-pressure and bar-pressure-m
# (named regions and a pressure load, in mm and in m, at 2 mesh sizes). The references are
# analytical. The script also checks that fh refuses the inputs of the M2b gate.
# Environment: PYTHON (python with build123d), FH_GMSH (gmsh program), KEEP=1 (keep the work folder),
# FH_WORK (use this folder as the work folder and keep it), FH_BIN (a folder with release fh and
# fh-solve, so no build), FH_EMULATOR (a command that runs fh, fh-solve, python and gmsh, for
# example "qemu-x86_64 -cpu Nehalem"; scripts/check-no-avx.sh sets it).
# With GNU time (Linux), each row also gives the elapsed time and the peak resident memory of the
# step, and times.tsv in the work folder keeps them. Under FH_EMULATOR, they are emulator values.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
python="${PYTHON:-python3}"
gmsh="${FH_GMSH:-gmsh}"
if [[ -n "${FH_WORK:-}" ]]; then
  work="$FH_WORK"
  mkdir -p "$work"
else
  work="$(mktemp -d "${TMPDIR:-/tmp}/fh-workflow.XXXXXX")"
  [[ "${KEEP:-0}" == 1 ]] || trap 'rm -rf "$work"' EXIT
fi
read -ra emulator <<<"${FH_EMULATOR:-}"
clock=native
[[ ${#emulator[@]} == 0 ]] || clock=emulator
timer=()
/usr/bin/time -v -o /dev/null true >/dev/null 2>&1 && timer=(/usr/bin/time -v -o)
printf 'Work folder: %s\n' "$work"
[[ $clock == native ]] || printf 'Emulator: %s. The times are emulator times, not hardware times.\n' "$FH_EMULATOR"
[[ ${#timer[@]} == 0 ]] || printf 'step\telapsed_s\tpeak_rss_mib\tclock\n' >"$work/times.tsv"
printf '%-30s %-6s %s\n' step result detail
failed=0
row() { printf '%-30s %-6s %s\n' "$1" "$2" "${3:-}"; }
# run NAME COMMAND...: run the command with its output in NAME.log. With GNU time, NAME.time
# keeps the time and the memory of the command.
run() {
  local name="$1"
  shift
  if [[ ${#timer[@]} == 0 ]]; then
    "$@" >"$work/$name.log" 2>&1
  else
    "${timer[@]}" "$work/$name.time" "$@" >"$work/$name.log" 2>&1
  fi
}
# used NAME: ", T s, M MiB" from NAME.time, and a row in times.tsv. Nothing without GNU time.
used() {
  [[ -f "$work/$1.time" ]] || return 0
  awk -F': ' -v name="$1" -v clock="$clock" -v tsv="$work/times.tsv" '
    /Elapsed \(wall clock\) time/ { n = split($2, t, ":"); s = 0; for (i = 1; i <= n; i++) s = s * 60 + t[i] }
    /Maximum resident set size/ { kib = $2 }
    END {
      printf "%s\t%.2f\t%.1f\t%s\n", name, s, kib / 1024, clock >>tsv
      printf ", %.2f s, %.1f MiB", s, kib / 1024
    }' "$work/$1.time"
}
# step NAME EXPECTED_EXIT COMMAND...: run the command, compare its exit code, keep its output in a log.
step() {
  local name="$1" want="$2"
  shift 2
  run "$name" "$@"
  local got=$?
  if [[ $got == "$want" ]]; then
    row "$name" PASS "exit $got$(used "$name")"
  else
    row "$name" FAIL "exit $got, expected $want (log: $work/$name.log)$(used "$name")"
    tail -n 5 "$work/$name.log"
    failed=1
  fi
  return $((got == want ? 0 : 1))
}
# refuse NAME CODE COMMAND...: the command must exit 1 and print the refusal code.
refuse() {
  local name="$1" code="$2"
  shift 2
  step "$name" 1 "$@" || return 1
  grep -q "$code" "$work/$name.log" || { row "$name" FAIL "no $code in the output"; failed=1; }
}
finish() {
  if [[ $failed == 0 ]]; then row workflow PASS; exit 0; fi
  row workflow FAIL
  exit 1
}
# emulate NAME PROGRAM: print the path of a wrapper that runs PROGRAM under FH_EMULATOR. The
# emulator loads binaries only, so a script runs through the interpreter of its #! line.
emulate() {
  local exe line wrapper="$work/emulated/$1"
  local interpreter=()
  exe="$(command -v "$2")" || return 1
  if [[ "$(head -c 2 "$exe")" == '#!' ]]; then
    IFS= read -r line <"$exe"
    read -ra interpreter <<<"${line#??}"
    if [[ ${interpreter[0]##*/} == env ]]; then
      interpreter=("$(command -v "${interpreter[1]}")" "${interpreter[@]:2}")
    fi
  fi
  mkdir -p "$work/emulated"
  {
    printf '#!/usr/bin/env bash\nexec'
    printf ' %q' "${emulator[@]}" "${interpreter[@]}" "$exe"
    printf ' "$@"\n'
  } >"$wrapper"
  chmod +x "$wrapper"
  printf '%s\n' "$wrapper"
}
tools=("$python" "$gmsh")
[[ -n "${FH_BIN:-}" ]] || tools+=(cargo)
[[ $clock == native ]] || tools+=("${emulator[0]}")
for tool in "${tools[@]}"; do
  command -v "$tool" >/dev/null 2>&1 || { row "$tool" FAIL "not found"; exit 1; }
done
"$python" -c 'import build123d' >/dev/null 2>&1 || { row build123d FAIL "python cannot import build123d"; exit 1; }
if [[ -n "${FH_BIN:-}" ]]; then
  target="$FH_BIN"
else
  step build 0 cargo build --release --bin fh --bin fh-solve || finish
  target="${CARGO_TARGET_DIR:-$root/target}/release"
fi
fh="$target/fh"
export FH_SOLVE="$target/fh-solve"
if [[ $clock == emulator ]]; then
  fh="$(emulate fh "$fh")"
  FH_SOLVE="$(emulate fh-solve "$FH_SOLVE")"
  python="$(emulate python "$python")"
  gmsh="$(emulate gmsh "$gmsh")"
fi

# copy_case NAME ROOT: a scratch repository root that holds the case without its mesh files.
copy_case() {
  local d="$2/validation/cases/$1"
  mkdir -p "$d"
  cp "validation/cases/$1/case.toml" "validation/cases/$1/derivation.md" "$d/" 2>/dev/null ||
    cp "validation/cases/$1/case.toml" "$d/"
  printf '%s\n' "$d"
}
# path NAME CASE_DIR UNIT SIZE: build123d STEP, unit check, mesh, mesh check, model, parse check.
path() {
  local name="$1" cdir="$2" unit="$3" size="$4"
  [[ -f "$cdir/bar.step" ]] || step "$name-cad" 0 "$python" scripts/cad/bar.py "$cdir/bar.step" "$unit" || return 1
  step "$name-step-units" 0 "$fh" info "$cdir/bar.step" --case "$cdir" || return 1
  step "$name-mesh" 0 "$fh" mesh "$cdir/bar.step" --case "$cdir" --size "$size" -o "$cdir/bar.msh" --gmsh "$gmsh" || return 1
  step "$name-mesh-quality" 0 "$fh" info "$cdir/bar.msh" --case "$cdir" || return 1
  step "$name-model" 0 "$fh" model "$cdir/bar.msh" --case "$cdir" -o "$cdir/bar.inp" || return 1
  step "$name-check-inp" 0 "$fh" check "$cdir/bar.inp"
}
# validate NAME ROOT: solve every case under ROOT and compare with the analytical references.
validate() {
  run "$1" "$fh" validate --root "$2"
  local code=$?
  printf '\n'
  cat "$work/$1.log"
  printf '\n'
  if [[ $code == 0 ]]; then row "$1" PASS "exit 0$(used "$1")"; else row "$1" FAIL "exit $code$(used "$1")"; failed=1; fi
}

# 1. bar-tension: plane node sets and an end displacement.
cdir="$(copy_case bar-tension "$work/root")"
path tension "$cdir" mm 8 || finish

# 2. bar-pressure: named regions and a pressure on a named face, in mm and in m, at 2 sizes.
# The second size gives a new mesh. It must keep the same named regions (fh model refuses a
# lost region), and its results must pass the same references.
# The coarse meshes go into the root of bar-tension, the fine meshes into a second root.
for size in coarse fine; do
  [[ $size == coarse ]] && base="$work/root" h=8 || base="$work/fine" h=4
  mm_dir="$(copy_case bar-pressure "$base")"
  m_dir="$(copy_case bar-pressure-m "$base")"
  path "pressure-mm-$size" "$mm_dir" mm "$h" || finish
  path "pressure-m-$size" "$m_dir" m "$(awk "BEGIN { print $h / 1000 }")" || finish
  n="$(grep -c '^\*SURFACE, TYPE=ELEMENT' "$mm_dir/bar.inp")"
  if [[ $n == 4 ]]; then row "regions-$size" PASS "4 element surfaces"; else row "regions-$size" FAIL "$n element surfaces"; failed=1; fi
done
grep -q '^\*\* units: SI-m$' "$work/fine/validation/cases/bar-pressure-m/bar.inp" ||
  { row units-in-input FAIL "no units line"; failed=1; }

# 3. Refusals of the M2b gate. Each must exit 1 with its code.
bad="$work/bad"
mkdir -p "$bad"
mm_dir="$work/fine/validation/cases/bar-pressure"
# A case with no units.
mkdir -p "$bad/nounits"
grep -v '^units = ' "$mm_dir/case.toml" >"$bad/nounits/case.toml"
cp "$mm_dir/derivation.md" "$bad/nounits/"
step refuse-no-units 1 "$fh" mesh "$mm_dir/bar.step" --case "$bad/nounits" -o "$bad/x.msh" --gmsh "$gmsh"
grep -q 'missing key `units`' "$work/refuse-no-units.log" || { row refuse-no-units FAIL "no missing units message"; failed=1; }
# The mm STEP file with a case in metres.
refuse refuse-unit-mismatch unit.step.mismatch "$fh" info "$mm_dir/bar.step" --case "$work/fine/validation/cases/bar-pressure-m"
# A STEP file with no length unit record.
grep -v 'LENGTH_UNIT' "$mm_dir/bar.step" >"$bad/nounit.step"
refuse refuse-step-no-unit unit.step.missing "$fh" info "$bad/nounit.step" --case "$mm_dir"
# A region that selects no face of the part (the CAD changed, or the plane is wrong).
mkdir -p "$bad/lost"
sed 's/^at = 100.0/at = 150.0/' "$mm_dir/case.toml" >"$bad/lost/case.toml"
cp "$mm_dir/derivation.md" "$bad/lost/"
refuse refuse-lost-region mesh.region.missing "$fh" mesh "$mm_dir/bar.step" --case "$bad/lost" --size 8 -o "$bad/lost.msh" --gmsh "$gmsh"
# A region renamed in the case after the mesh was made.
mkdir -p "$bad/renamed"
sed 's/"LOAD"/"PULL"/' "$mm_dir/case.toml" >"$bad/renamed/case.toml"
cp "$mm_dir/derivation.md" "$bad/renamed/"
cp "$mm_dir/bar.msh" "$mm_dir/bar.msh.units" "$bad/renamed/"
refuse refuse-renamed-region mesh.region.missing "$fh" model "$bad/renamed/bar.msh" --case "$bad/renamed" -o "$bad/renamed/bar.inp"
# A volume with no physical group: gmsh writes none of its elements. The edit sets the physical
# tag count of each volume entity to 0.
awk '/^\$Entities/ { e = 1; print; next }
  e == 1 { left = $1 + $2 + $3; e = 2; print; next }
  e == 2 && /^\$EndEntities/ { e = 0 }
  e == 2 && left > 0 { left--; print; next }
  e == 2 { out = $1; for (i = 2; i <= 7; i++) out = out " " $i; out = out " 0"
           for (i = 9 + $8; i <= NF; i++) out = out " " $i; print out; next }
  { print }' "$mm_dir/bar.msh" >"$bad/renamed/unnamed.msh"
cp "$mm_dir/bar.msh.units" "$bad/renamed/unnamed.msh.units"
refuse refuse-unnamed-volume mesh.volume.unnamed "$fh" model "$bad/renamed/unnamed.msh" --case "$mm_dir" -o "$bad/unnamed.inp"
# A failure of the external program: a gmsh that reports a version and then fails.
printf '#!/bin/sh\n[ "$1" = -version ] && { echo 4.15.2; exit 0; }\necho "Error   : the mesher failed" >&2\nexit 1\n' >"$bad/gmsh"
chmod +x "$bad/gmsh"
refuse refuse-gmsh-failure "gmsh failed" "$fh" mesh "$mm_dir/bar.step" --case "$mm_dir" -o "$bad/x.msh" --gmsh "$bad/gmsh"

# 4. Solve and compare with the analytical references. Each size of the pressure cases runs
# in mm and in m, so the same physical answer must come out of both unit systems.
validate solve-validate-coarse "$work/root"
validate solve-validate-fine "$work/fine"
finish
