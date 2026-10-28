#!/usr/bin/env bash
# Build 1 HTML file for each validation case in CASES with quine-wasm (docs/reference/quine.md). Each file
# holds the case, the native solvers of femahoy (crates/quine) and, after a run, the results and
# the check against the reference. With --test, open each file in Chromium and check it
# (scripts/quine/quine.spec.js).
#
# Needs emcc (nix shell nixpkgs#emscripten), Node and git. quine-wasm is cloned at QUINE_WASM_REV
# into the cargo target folder; femahoy holds none of its code.
#
# Usage: scripts/build-quine.sh [--test] [OUT]   (OUT default: target/quine)
set -euo pipefail

# The cases: each needs only the native solvers, and each has an analytic reference and a
# tolerance budget in its check.toml. Heat, stress, a 2D line, a 3D waveguide and a unit cell.
CASES="grid-heated-bar grid-kirsch-hole line-coax grid-waveguide-te10 grid-laminate-cell"
QUINE_WASM_REV=21d6f08ddbd46fa2fe5dd198a5435e91c7dbb6fa

root=$(cd "$(dirname "$0")/.." && pwd)
abs() { case $1 in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
test=0
if [ "${1:-}" = --test ]; then test=1; shift; fi
target=$(abs "${CARGO_TARGET_DIR:-$root/target}")
out=$(abs "${1:-$target/quine}")
qw=$target/quine-wasm-$QUINE_WASM_REV

if ! command -v emcc >/dev/null; then
  echo "build-quine: emcc not found: run in nix shell nixpkgs#emscripten nixpkgs#nodejs" >&2
  exit 1
fi
if [ ! -d "$qw/node_modules" ]; then
  rm -rf "$qw"
  git clone --quiet https://github.com/yujieteo/quine-wasm "$qw"
  git -C "$qw" checkout --quiet "$QUINE_WASM_REV"
  (cd "$qw" && npm ci --silent)
fi

# rustup reads rust-toolchain.toml in the current folder.
cd "$root"
CARGO_TARGET_DIR=$target cargo build -p fh-quine --release --target wasm32-unknown-emscripten
lib=$target/wasm32-unknown-emscripten/release/libfh_quine.a
mkdir -p "$out"
for case in $CASES; do
  seed=$(mktemp -d)
  cp -R "$root/validation/cases/$case" "$seed/"
  node "$qw/bin/quine-wasm.js" -o "$out/$case.html" --title "$case" --args "fh-quine $case" \
    --seed "$seed" -- -O2 "$lib"
  rm -rf "$seed"
done
ls -l "$out"/*.html

if [ "$test" = 1 ]; then
  (cd "$qw" && npx playwright install chromium)
  # The test uses the Playwright of the quine-wasm clone, so femahoy has no Node dependency.
  FH_QUINE_OUT=$out FH_QUINE_CASES=$CASES NODE_PATH=$qw/node_modules \
    "$qw/node_modules/.bin/playwright" test --config "$root/scripts/quine/playwright.config.js"
fi
