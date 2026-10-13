#!/usr/bin/env bash
# Manual Linux check. Emulate both the CLI and every solver subprocess.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  echo 'FAIL: use an x86-64 Linux host or an amd64 Linux container.' >&2
  exit 1
fi
for tool in cargo cc qemu-x86_64; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "FAIL: $tool is not installed or not in PATH." >&2
    exit 1
  fi
done
work="$(mktemp -d "${TMPDIR:-/tmp}/fh-no-avx.XXXXXX")"
trap 'rm -rf "$work"' EXIT
ulimit -c 0
cat > "$work/avx.c" <<'C'
int main(void) {
    __asm__ volatile ("vzeroupper");
    return 0;
}
C
cc -march=x86-64 -o "$work/avx" "$work/avx.c"
status=0
qemu-x86_64 -cpu Nehalem "$work/avx" || status=$?
if [[ "$status" != 132 ]]; then
  echo "FAIL: the AVX probe exited with $status. Expected SIGILL (132)." >&2
  exit 1
fi
echo 'PASS: Nehalem rejects AVX with SIGILL.'
env -u CARGO_ENCODED_RUSTFLAGS \
  RUSTFLAGS='-C target-cpu=x86-64 -C target-feature=+sse4.2,-avx,-avx2' \
  cargo build --release --target x86_64-unknown-linux-gnu --target-dir "$work/target" --bin fh --bin fh-solve
export FH_NO_AVX_SOLVER="$work/target/x86_64-unknown-linux-gnu/release/fh-solve"
cat > "$work/fh-solve-qemu" <<'SH'
#!/bin/sh
exec qemu-x86_64 -cpu Nehalem "$FH_NO_AVX_SOLVER" "$@"
SH
chmod +x "$work/fh-solve-qemu"
export FH_SOLVE="$work/fh-solve-qemu"
qemu-x86_64 -cpu Nehalem "$work/target/x86_64-unknown-linux-gnu/release/fh" validate
echo 'PASS: fh validate and each solver subprocess ran under Nehalem.'
