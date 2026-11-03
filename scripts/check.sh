#!/usr/bin/env bash
# The static checks: clippy with the lints of the workspace (Cargo.toml) for the host and for the
# wasm viewer, ruff (ruff.toml) and pyright (pyrightconfig.json) on the Python scripts.
# A warning fails the check. Run it before a commit. CI does not run it.
#
# Needs ruff and pyright on PATH. Without them, the script runs itself again in a Nix shell
# that has them.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
if ! command -v ruff >/dev/null || ! command -v pyright >/dev/null; then
    if [ -n "${FH_CHECK_NIX:-}" ]; then
        echo "check.sh: no ruff or pyright on PATH, also in the Nix shell" >&2
        exit 1
    fi
    FH_CHECK_NIX=1 exec nix shell nixpkgs#ruff nixpkgs#pyright -c "$0" "$@"
fi
# rustup reads rust-toolchain.toml in the current folder.
cd "$root"
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p fh-viewer --lib --target wasm32-unknown-unknown -- -D warnings
ruff check .
pyright
