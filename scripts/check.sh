#!/usr/bin/env bash
# The static checks: clippy with the lints of the workspace (Cargo.toml) for the host and for the
# wasm viewer, ruff (ruff.toml) and pyright (pyrightconfig.json) on the Python scripts, and
# the pinned shellcheck 0.11.0 with every optional rule (-o all) on the shell scripts: the committed files
# that end in .sh and those whose first line names sh, bash or ksh.
# A warning fails the check. Run it before a commit. CI does not run it.
#
# Needs ruff, pyright and uvx on PATH (uvx fetches the pinned shellcheck). Without them, the script runs itself again in a Nix shell
# that has them.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
if ! command -v ruff >/dev/null || ! command -v pyright >/dev/null || ! command -v uvx >/dev/null; then
    if [[ -n "${FH_CHECK_NIX:-}" ]]; then
        echo "check.sh: no ruff, pyright or uvx on PATH, also in the Nix shell" >&2
        exit 1
    fi
    FH_CHECK_NIX=1 exec nix shell nixpkgs#ruff nixpkgs#pyright nixpkgs#uv -c "$0" "$@"
fi
# rustup reads rust-toolchain.toml in the current folder.
cd "${root}"
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p fh-viewer --lib --target wasm32-unknown-unknown -- -D warnings
ruff check .
pyright

tracked=$(mktemp)
trap 'rm -f "${tracked}"' EXIT
git ls-files -z >"${tracked}"
shell_files=()
while IFS= read -r -d '' f; do
    first=$(head -n 1 "${f}" | LC_ALL=C tr -d '\0' || true)
    if [[ ${f} == *.sh ]] || [[ ${first} =~ ^#!(.*[^[:alnum:]_])?(sh|bash|ksh)([^[:alnum:]_]|$) ]]; then
        shell_files+=("${f}")
    fi
done <"${tracked}"
uvx --from shellcheck-py==0.11.0.1 shellcheck -o all "${shell_files[@]}"
