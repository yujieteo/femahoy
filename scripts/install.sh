#!/usr/bin/env bash
# Install femahoy from this repository: build `fh` and the wasm viewer, and copy them to one
# folder. Each solve needs CalculiX (`ccx`), see docs/ccx.md.
# Make the Python environment of the CAD step with uv.
#
#   scripts/install.sh [--prefix DIR] [--venv DIR]
#
# The prefix (default ~/.local/bin) gets fh, the folder viewer beside fh, where
# `fh serve` looks for it, and fh-python. fh-python runs the Python of the venv (default
# ${XDG_DATA_HOME:-~/.local/share}/femahoy/venv), which has Python 3.12 and build123d.
# A second run builds again and updates the files and the venv. It removes the fh-solve and
# fh-solve-ccx of an earlier install: fh starts ccx directly now.
#
# The script finds cargo in ~/.cargo/bin ($CARGO_HOME/bin) when cargo is not on PATH. It
# checks all the tools before the build and shows the install command of each missing tool.
# It installs no tool and downloads no Rust toolchain (RUSTUP_AUTO_INSTALL=0). uv downloads
# Python 3.12 and build123d when they are not in its cache.
set -euo pipefail

usage="usage: scripts/install.sh [--prefix DIR] [--venv DIR]"
prefix=$HOME/.local/bin
venv=${XDG_DATA_HOME:-$HOME/.local/share}/femahoy/venv
while [ $# -gt 0 ]; do
  case $1 in
    --prefix | --venv)
      if [ $# -lt 2 ] || [ -z "$2" ]; then
        printf 'install: %s needs a folder\n%s\n' "$1" "$usage" >&2
        exit 2
      fi
      if [ "$1" = --prefix ]; then prefix=$2; else venv=$2; fi
      shift 2
      ;;
    --prefix=?*) prefix=${1#--prefix=}; shift ;;
    --venv=?*) venv=${1#--venv=}; shift ;;
    -h | --help) echo "$usage"; exit 0 ;;
    *) printf 'install: unknown argument: %s\n%s\n' "$1" "$usage" >&2; exit 2 ;;
  esac
done
case $prefix in /*) ;; *) prefix=$PWD/$prefix ;; esac
case $venv in /*) ;; *) venv=$PWD/$venv ;; esac

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
if ! command -v uv >/dev/null 2>&1; then
  missing+=("uv (it makes the Python environment of the CAD step): curl -LsSf https://astral.sh/uv/install.sh | sh")
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

echo "install: build fh (release)"
cargo build --release --locked --bin fh
echo "install: build the wasm viewer"
scripts/build-viewer.sh "$target/release/viewer"
# The tested version of docs/cad-recipe.md.
echo "install: make the Python environment with build123d in $venv"
uv venv --quiet --allow-existing -p 3.12 "$venv"
uv pip install --quiet -p "$venv/bin/python" build123d==0.13.0

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
put "$target/release/fh$exe" "$prefix/fh$exe" 755
rm -f "$prefix/fh-solve$exe" "$prefix/fh-solve-ccx$exe"
for file in fh_viewer.js fh_viewer_bg.wasm; do
  put "$target/release/viewer/$file" "$prefix/viewer/$file" 644
done
# The venv path in single quotes, with each quote in it as '\''.
printf "#!/bin/sh\nexec '%s/bin/python' \"\$@\"\n" "${venv//\'/\'\\\'\'}" >"$target/release/fh-python"
put "$target/release/fh-python" "$prefix/fh-python" 755

if [ $update = yes ]; then verb=updated; else verb=installed; fi
echo "install: $verb in $prefix:"
for file in "fh$exe" fh-python viewer/fh_viewer.js viewer/fh_viewer_bg.wasm; do
  echo "  $prefix/$file"
done
echo "install: fh-python runs the Python of $venv, with build123d: fh-python part.py"
if [ $update = yes ]; then
  echo "install: if a daemon runs, it keeps the earlier fh until it stops. Stop it with: fh serve --stop"
fi
if ! command -v "${FH_CCX:-ccx}" >/dev/null 2>&1; then
  echo "install: ccx (CalculiX) is not installed. Each solve needs it: apt install calculix-ccx (Debian, Ubuntu), or see docs/ccx.md"
fi
if ! command -v gmsh >/dev/null 2>&1 && [ ! -x /Applications/Gmsh.app/Contents/MacOS/gmsh ]; then
  echo "install: gmsh is not installed. fh mesh needs it for a STEP file: brew install gmsh (macOS), or see https://gmsh.info"
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
run="fh serve"
if ! on_path "$prefix"; then
  run="$prefix/fh serve"
  echo "install: $prefix is not on PATH. To run fh by its name, add this line to your shell start file (for example ~/.zshrc):"
  echo "  export PATH=\"$prefix:\$PATH\""
fi
echo "install: next, start the page and write a request in the chat:"
echo "  $run"
