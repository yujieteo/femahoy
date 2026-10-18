#!/usr/bin/env bash
# Build the wasm viewer that `fh serve` serves (crates/viewer, docs/viewer.md).
#
# Needs the Rust target wasm32-unknown-unknown and wasm-bindgen-cli at the version of
# wasm-bindgen in Cargo.lock:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <version> --locked
#
# Writes fh_viewer.js and fh_viewer_bg.wasm to OUT (default release/viewer in the cargo
# target folder, $CARGO_TARGET_DIR or target, beside `fh` where `fh serve` looks). Set FH_VIEWER=OUT for another folder.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
abs() { case $1 in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
target=$(abs "${CARGO_TARGET_DIR:-$root/target}")
out=$(abs "${1:-$target/release/viewer}")
# rustup reads rust-toolchain.toml in the current folder.
cd "$root"
want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print }' "$root/Cargo.lock")
have=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }') || true
if [ "$have" != "$want" ]; then
  echo "build-viewer: wasm-bindgen ${have:-not found}, need $want:" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

CARGO_TARGET_DIR=$target cargo build -p fh-viewer --lib --release --target wasm32-unknown-unknown
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-name fh_viewer --out-dir "$out" \
  "$target/wasm32-unknown-unknown/release/fh_viewer.wasm"
ls -l "$out/fh_viewer.js" "$out/fh_viewer_bg.wasm"
