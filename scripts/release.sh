#!/usr/bin/env bash
# Tag femahoy as vVERSION and make its GitHub release with the fh builds of this computer.
#
#   scripts/release.sh VERSION      (for example scripts/release.sh 0.1.0)
#
# The script stops, before it builds, when VERSION, the version in Cargo.toml and the top entry
# of CHANGELOG.md do not agree, or when the tree has changes. A new tag must be at origin/main.
# Thus the release is the merged code and the notes are the CHANGELOG.md entry.
#
# Each asset is fh-VERSION-ARCH-OS.tar.gz with fh and the folder viewer beside it, where
# `fh serve` looks for it. The solvers are not in it (scripts/install.sh installs them).
# - On Linux, the script builds x86_64-linux and aarch64-linux. Each fh is a static musl
#   program, so it runs on each Linux. rust-lld links it, so the build needs no C compiler.
# - On macOS, the script builds aarch64-macos.
#
# When the tag already exists, the script must run at the tag. It then adds its assets to the
# release. Thus a Mac adds the macOS asset to a release
# that Linux made: git checkout vVERSION, then scripts/release.sh VERSION.
#
# Needs git, gh-axi with a login, rustup, and wasm-bindgen-cli for the viewer
# (scripts/build-viewer.sh).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
fail() { echo "release: $*" >&2; exit 1; }

[ $# -eq 1 ] || { echo "usage: scripts/release.sh VERSION" >&2; exit 2; }
version=$1
tag=v$version
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "VERSION must be X.Y.Z, not $version"

cargo_version=$(awk '/^\[workspace.package\]$/ { p = 1 } p && /^version = / { gsub(/version = |"/, ""); print; exit }' Cargo.toml)
changelog_version=$(awk '/^## / { print $2; exit }' CHANGELOG.md)
[ "$cargo_version" = "$version" ] || fail "Cargo.toml has version $cargo_version, not $version"
[ "$changelog_version" = "$version" ] || fail "the top entry of CHANGELOG.md is $changelog_version, not $version"
[ -z "$(git status --porcelain)" ] || fail "the tree has changes: commit or remove them"

git fetch --quiet --tags origin main
head=$(git rev-parse HEAD)
if git rev-parse --quiet --verify "refs/tags/$tag" >/dev/null; then
  [ "$(git rev-parse "$tag^{commit}")" = "$head" ] || fail "$tag exists and HEAD is not at it: git checkout $tag"
  new_tag=
else
  [ "$(git rev-parse origin/main)" = "$head" ] || fail "HEAD is not at origin/main: git checkout origin/main"
  new_tag=1
fi

case $(uname -s) in
  Linux) targets="x86_64-unknown-linux-musl aarch64-unknown-linux-musl" ;;
  Darwin) targets="aarch64-apple-darwin" ;;
  *) fail "no build for $(uname -s)" ;;
esac

abs() { case $1 in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
target_dir=$(abs "${CARGO_TARGET_DIR:-$root/target}")
dist=$target_dir/dist/$tag
rm -rf "$dist"
mkdir -p "$dist"
scripts/build-viewer.sh "$dist/viewer"

assets=()
for target in $targets; do
  rustup target add "$target"
  linker=CARGO_TARGET_$(echo "$target" | tr a-z- A-Z_)_LINKER
  case $target in
    *-linux-musl) export "$linker=rust-lld" ;;
  esac
  CARGO_TARGET_DIR=$target_dir cargo build --release --locked --bin fh --target "$target"
  arch=${target%%-*}
  case $target in
    *-linux-*) os=linux ;;
    *-apple-*) os=macos ;;
  esac
  name=fh-$version-$arch-$os
  mkdir -p "$dist/$name"
  cp "$target_dir/$target/release/fh" "$dist/$name/fh"
  cp -R "$dist/viewer" "$dist/$name/viewer"
  # The fh of this computer must print the release version.
  if [ "$arch" = "$(uname -m | sed s/arm64/aarch64/)" ]; then
    printed=$("$dist/$name/fh" --version)
    [ "$printed" = "fh $version" ] || fail "$name/fh --version printed '$printed'"
  fi
  tar -C "$dist" -czf "$dist/$name.tar.gz" "$name"
  assets+=("$dist/$name.tar.gz")
done

# The notes are the lines of the top entry of CHANGELOG.md, below its heading.
awk '/^## / { n++; next } n == 1' CHANGELOG.md >"$dist/notes.md"

if [ -n "$new_tag" ]; then
  git tag -a "$tag" -m "femahoy $version"
  git push origin "$tag"
fi
if gh-axi release view "$tag" >/dev/null 2>&1; then
  gh-axi release upload "$tag" "${assets[@]}"
else
  gh-axi release create "$tag" --verify-tag --title "femahoy $version" --notes-file "$dist/notes.md" "${assets[@]}"
fi
gh-axi release view "$tag"
