#!/usr/bin/env bash
# Install femahoy from this repository: build `fh`, `fh-solve` and the wasm viewer, and copy
# them to one folder.
#
#   scripts/install.sh [--prefix DIR]
#
# DIR (default ~/.local/bin) gets fh, fh-solve, and the folder viewer beside fh, where
# `fh serve` looks for it. A second run builds again and updates the files.
#
# The script finds cargo in ~/.cargo/bin ($CARGO_HOME/bin) when cargo is not on PATH. It
# checks all the tools before the build and shows the install command of each missing tool.
# It installs no tool and downloads no toolchain (RUSTUP_AUTO_INSTALL=0).
set -euo pipefail

usage="usage: scripts/install.sh [--prefix DIR]"
prefix=$HOME/.local/bin
while [ $# -gt 0 ]; do
  case $1 in
    --prefix)
      if [ $# -lt 2 ] || [ -z "$2" ]; then
        printf 'install: --prefix needs a folder\n%s\n' "$usage" >&2
        exit 2
      fi
      prefix=$2
      shift 2
      ;;
    --prefix=?*) prefix=${1#--prefix=}; shift ;;
    -h | --help) echo "$usage"; exit 0 ;;
    *) printf 'install: unknown argument: %s\n%s\n' "$1" "$usage" >&2; exit 2 ;;
  esac
done
case $prefix in /*) ;; *) prefix=$PWD/$prefix ;; esac

root=$(cd "$(dirname "$0")/.." && pwd)
# rustup reads rust-toolchain.toml in the current folder.
cd "$root"
export RUSTUP_AUTO_INSTALL=0
cargo_bin=${CARGO_HOME:-$HOME/.cargo}/bin
if [ -d "$cargo_bin" ] && [[ ":$PATH:" != *":$cargo_bin:"* ]]; then
  PATH=$PATH:$cargo_bin
fi
target=${CARGO_TARGET_DIR:-target}
case $target in /*) ;; *) target=$root/$target ;; esac

toolchain=$(awk -F'"' '/^channel *=/ { print $2 }' rust-toolchain.toml)
wasm=wasm32-unknown-unknown
bindgen=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print }' Cargo.lock)

# Check the tools. Each missing tool adds a line with its install command.
missing=()
if ! command -v cargo >/dev/null 2>&1; then
  missing+=("Rust (cargo and rustup): curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --default-toolchain none")
elif command -v rustup >/dev/null 2>&1; then
  if ! rustup toolchain list 2>/dev/null | grep -q "^$toolchain-"; then
    missing+=("Rust $toolchain: rustup toolchain install $toolchain --profile minimal --component rustfmt,clippy --target $wasm")
  else
    installed=$(rustup component list --toolchain "$toolchain" --installed 2>/dev/null || true)
    for component in rustfmt clippy; do
      if ! grep -q "^$component-" <<<"$installed"; then
        missing+=("$component of Rust $toolchain: rustup component add $component --toolchain $toolchain")
      fi
    done
    if ! grep -qx "rust-std-$wasm" <<<"$installed"; then
      missing+=("the $wasm target of Rust $toolchain: rustup target add $wasm --toolchain $toolchain")
    fi
  fi
elif [ ! -d "$(rustc --print sysroot 2>/dev/null)/lib/rustlib/$wasm" ]; then
  # A Rust with no rustup does not read rust-toolchain.toml.
  missing+=("the $wasm target of your Rust: with rustup: rustup target add $wasm")
fi
have=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }') || true
if [ "$have" != "$bindgen" ]; then
  missing+=("wasm-bindgen $bindgen (found: ${have:-none}): cargo install wasm-bindgen-cli --version $bindgen --locked")
fi
if [ "$(uname)" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then
  missing+=("the Xcode command line tools (the C linker): xcode-select --install")
elif ! command -v cc >/dev/null 2>&1; then
  missing+=("a C compiler and linker (cc): install it from your package manager")
fi
if [ ${#missing[@]} -gt 0 ]; then
  echo "install: ${#missing[@]} tool(s) are missing. Install them, then run scripts/install.sh again:" >&2
  for line in "${missing[@]}"; do
    echo "  - $line" >&2
  done
  exit 1
fi

echo "install: build fh and fh-solve (release)"
cargo build --release --locked --bin fh --bin fh-solve
echo "install: build the wasm viewer"
scripts/build-viewer.sh "$target/release/viewer"

# Copy to a new file in the folder, then rename it. A rename does not change a file that a
# running program uses, and macOS does not stop a new program for a changed signature.
put() {
  local from=$1 to=$2 mode=$3
  cp "$from" "$to.new.$$"
  chmod "$mode" "$to.new.$$"
  mv -f "$to.new.$$" "$to"
}
update=no
if [ -e "$prefix/fh" ]; then update=yes; fi
mkdir -p "$prefix/viewer"
exe=
if [[ $(uname -s) == MINGW* || $(uname -s) == MSYS* || $(uname -s) == CYGWIN* ]]; then exe=.exe; fi
for bin in fh fh-solve; do
  put "$target/release/$bin$exe" "$prefix/$bin$exe" 755
done
for file in fh_viewer.js fh_viewer_bg.wasm; do
  put "$target/release/viewer/$file" "$prefix/viewer/$file" 644
done

if [ $update = yes ]; then verb=updated; else verb=installed; fi
echo "install: $verb in $prefix:"
for file in "fh$exe" "fh-solve$exe" viewer/fh_viewer.js viewer/fh_viewer_bg.wasm; do
  echo "  $prefix/$file"
done
if [ $update = yes ]; then
  echo "install: if a daemon runs, it keeps the earlier fh until it stops. Stop it with: fh serve --stop"
fi
if ! command -v gmsh >/dev/null 2>&1 && [ ! -x /Applications/Gmsh.app/Contents/MacOS/gmsh ]; then
  echo "install: gmsh is not installed. fh run needs it for a case folder and the example: brew install gmsh (macOS), or see https://gmsh.info"
fi
# True when a folder on PATH is the folder $1, also by another name (a symbolic link).
on_path() {
  local want dir IFS=:
  want=$(cd "$1" && pwd -P)
  for dir in $PATH; do
    if [ -n "$dir" ] && [ "$(cd "$dir" 2>/dev/null && pwd -P)" = "$want" ]; then return 0; fi
  done
  return 1
}
run="fh run --example bar-pressure"
if ! on_path "$prefix"; then
  run="$prefix/fh run --example bar-pressure"
  echo "install: $prefix is not on PATH. To run fh by its name, add this line to your shell start file (for example ~/.zshrc):"
  echo "  export PATH=\"$prefix:\$PATH\""
fi
echo "install: next, solve the example and see the results in the browser:"
echo "  $run"
