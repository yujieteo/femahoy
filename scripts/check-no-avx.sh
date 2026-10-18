#!/usr/bin/env bash
# Linux check of the target hardware and determinism (E3). QEMU user mode with the Nehalem CPU
# model (SSE4.2, no AVX) runs the CLI, every solver subprocess and the external programs.
# 1. An AVX probe must fail with SIGILL.
# 2. Release fh and fh-solve for x86-64 with SSE4.2 and no AVX.
# 3. fh validate on the validation cases.
# 4. The CLI workflow of scripts/check-workflow.sh: build123d (Python), fh, gmsh and fh-solve.
# 5. scripts/check-threads.py: the result bytes of fh-solve at 1, 2, 4 and 6 threads.
# 6. The matrix bytes at 1, 2, 4 and 6 threads: a unit test of core-solver, because the solver
#    writes no matrix file. The test binary has the same target and RUSTFLAGS as step 2.
# 7. The CPU features that the external programs declare or find.
# Environment: PYTHON and FH_GMSH as in check-workflow.sh. FH_CHECK_OUT: a new folder that keeps
# the logs and tables. Without it, the script uses a temporary folder and removes it.
# The times are emulator times, not hardware times.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  echo 'FAIL: use an x86-64 Linux host or an amd64 Linux container.' >&2
  exit 1
fi
python="${PYTHON:-python3}"
gmsh="${FH_GMSH:-gmsh}"
for tool in cargo cc qemu-x86_64 "$python" "$gmsh"; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "FAIL: $tool is not installed or not in PATH." >&2
    exit 1
  fi
done
emulator=(qemu-x86_64 -cpu Nehalem)
work="$(mktemp -d "${TMPDIR:-/tmp}/fh-no-avx.XXXXXX")"
trap 'rm -rf "$work"' EXIT
out="${FH_CHECK_OUT:-$work/out}"
mkdir "$out"
ulimit -c 0
{
  printf 'date: %s\n' "$(date -u +%Y-%m-%dT%H:%MZ)"
  printf 'commit: %s\n' "$(git log -1 --format='%H %s' 2>/dev/null || echo unknown)"
  printf 'cpu: %s\n' "$(grep -m1 '^model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')"
  printf 'host avx flags: %s\n' "$(grep -m1 '^flags' /proc/cpuinfo | grep -ow 'avx[a-z0-9_]*' | tr '\n' ' ')"
  printf 'cores: %s\n' "$(nproc)"
  printf 'memory: %s\n' "$(grep -m1 MemTotal /proc/meminfo | tr -s ' ' | cut -d' ' -f2-)"
  printf 'emulator: %s (%s)\n' "${emulator[*]}" "$(qemu-x86_64 --version | head -n 1)"
  printf 'rust: %s\n' "$(rustc --version)"
  printf 'gmsh: %s\n' "$("$gmsh" -version 2>&1 | tail -n 1)"
  printf 'python: %s\n' "$("$python" --version 2>&1)"
  printf 'python packages: %s\n' "$("$python" -c 'import importlib.metadata as m
print(" ".join(sorted(f"{d.name}=={d.version}" for d in m.distributions())))')"
} | tee "$out/host.txt"

# 1. AVX probe.
cat > "$work/avx.c" <<'C'
int main(void) {
    __asm__ volatile ("vzeroupper");
    return 0;
}
C
cc -march=x86-64 -o "$work/avx" "$work/avx.c"
status=0
"${emulator[@]}" "$work/avx" || status=$?
if [[ "$status" != 132 ]]; then
  echo "FAIL: the AVX probe exited with $status. Expected SIGILL (132)." >&2
  exit 1
fi
echo 'PASS: Nehalem rejects AVX with SIGILL.'

# 2. Release build with SSE4.2 and no AVX.
no_avx_cargo() {
  env -u CARGO_ENCODED_RUSTFLAGS \
    RUSTFLAGS='-C target-cpu=x86-64 -C target-feature=+sse4.2,-avx,-avx2' \
    cargo "$1" --release --target x86_64-unknown-linux-gnu --target-dir "$work/target" "${@:2}"
}
no_avx_cargo build --bin fh --bin fh-solve
bin="$work/target/x86_64-unknown-linux-gnu/release"

# 3. fh validate. Each solver subprocess also runs under the emulator.
export FH_NO_AVX_SOLVER="$bin/fh-solve"
cat > "$work/fh-solve-qemu" <<'SH'
#!/bin/sh
exec qemu-x86_64 -cpu Nehalem "$FH_NO_AVX_SOLVER" "$@"
SH
chmod +x "$work/fh-solve-qemu"
timer=()
if /usr/bin/time -v -o /dev/null true >/dev/null 2>&1; then
  timer=(/usr/bin/time -v -o "$out/validate.time")
fi
FH_SOLVE="$work/fh-solve-qemu" "${timer[@]}" "${emulator[@]}" "$bin/fh" validate
[[ ! -f "$out/validate.time" ]] || grep -E 'Elapsed|Maximum resident' "$out/validate.time"
echo 'PASS: fh validate and each solver subprocess ran under Nehalem.'

# 4. The CLI workflow. check-workflow.sh runs every program under the emulator.
FH_BIN="$bin" FH_EMULATOR="${emulator[*]}" FH_WORK="$out/workflow" PYTHON="$python" FH_GMSH="$gmsh" \
  scripts/check-workflow.sh
echo 'PASS: the CLI workflow ran under Nehalem.'

# 5. Result bytes at 1, 2, 4 and 6 threads: 2 inputs of the workflow, a box for Cholesky and a
# box for PCG.
cases="$out/workflow"
"$python" scripts/check-threads.py --solver "$bin/fh-solve" --emulator "${emulator[*]}" \
  --output "$out/threads" --threads 1,2,4,6 --box 14 --box 28 \
  "$cases/root/validation/cases/bar-tension/bar.inp" "$cases/fine/validation/cases/bar-pressure/bar.inp"

# 6. The matrix bytes. Cargo runs the test binary under the emulator.
test_timer=()
[[ ${#timer[@]} == 0 ]] || test_timer=(/usr/bin/time -v -o "$out/matrix.time")
printf '#!/usr/bin/env bash\nexec %s"$@"\n' "$(printf '%q ' "${test_timer[@]}" "${emulator[@]}")" > "$work/test-runner"
chmod +x "$work/test-runner"
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="$work/test-runner" no_avx_cargo test -p fh-core-solver \
  --lib -- --exact step::tests::the_matrix_has_the_same_bits_for_any_thread_count 2>&1 | tee "$out/matrix.log"
[[ ! -f "$out/matrix.time" ]] || grep -E 'Elapsed|Maximum resident' "$out/matrix.time"
grep -q '^test result: ok. 1 passed' "$out/matrix.log"
echo 'PASS: the same matrix bytes at 1, 2, 4 and 6 threads under Nehalem.'

# 7. External programs. Step 4 ran them under Nehalem. Record the CPU features that numpy (a
# build123d dependency) finds under Nehalem, and the x86 ISA level that each ELF file declares.
{
  echo '## numpy under Nehalem'
  "${emulator[@]}" "$(command -v "$python")" -c 'import numpy; numpy.show_runtime()' 2>&1
  echo '## x86 ISA needed (readelf -n): count and value'
  {
    "$python" -c 'import os, sys; print(os.path.realpath(sys.executable))'
    "$python" -c 'import gmsh; print(gmsh.libpath)' 2>/dev/null || command -v "$gmsh"
    find "$("$python" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')" -name '*.so*' -type f
  } | while read -r f; do
    note="$(readelf -n "$f" 2>/dev/null | grep -m1 'x86 ISA needed' | sed 's/.*needed: *//')" || true
    printf '%s\t%s\n' "${note:-no ISA note}" "$f"
  done | tee "$out/isa.tsv" | cut -f1 | sort | uniq -c
} | tee "$out/external.txt"
echo "PASS: E3 checks under ${emulator[*]}. Logs and tables: $out"
