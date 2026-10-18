#!/usr/bin/env bash
# Counts Rust lines per crate with tokei, tests included, and fails when a
# crate is over its budget (DESIGN.md section 11).
set -euo pipefail

# Budgets in lines per crate, from DESIGN.md section 11. Format: crate:budget.
# cli is 4800, up from 3000, and daemon is 7300, up from 7000, for fh run: the captain
# approved it (DESIGN.md section 11).
BUDGETS="
inp:9000
io:5000
toon:3000
solver-api:3000
core-solver:30000
validation:6000
daemon:7300
agents:4000
cli:4800
viewer:15000
"

# Total cap across all crates, from DESIGN.md section 11.
TOTAL_BUDGET=85000

root="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v tokei >/dev/null 2>&1; then
  echo "check-budget: tokei is not installed or not in PATH. Install it, for example: cargo install --locked tokei" >&2
  exit 1
fi

status=0
total=0
for entry in $BUDGETS; do
  crate="${entry%%:*}"
  budget="${entry##*:}"
  dir="$root/crates/$crate"

  if [ ! -d "$dir" ]; then
    echo "check-budget: crate directory missing: crates/$crate" >&2
    status=1
    continue
  fi

  # The Total row of the Rust report has the Lines column (code, comments and
  # blanks, including doc comments) in field 3.
  lines="$(tokei -t Rust "$dir" | awk '$1 == "Total" { print $3 }')"
  case "$lines" in
    ''|*[!0-9]*)
      echo "check-budget: could not read a line count for crate $crate from tokei" >&2
      exit 1
      ;;
  esac

  total=$((total + lines))
  if [ "$lines" -gt "$budget" ]; then
    echo "OVER  $crate: $lines lines > budget $budget"
    status=1
  else
    echo "ok    $crate: $lines lines <= budget $budget"
  fi
done

echo "total: $total lines"
if [ "$total" -gt "$TOTAL_BUDGET" ]; then
  echo "OVER  total: $total lines > total budget $TOTAL_BUDGET"
  status=1
else
  echo "ok    total: $total lines <= total budget $TOTAL_BUDGET"
fi
if [ "$status" -ne 0 ]; then
  echo "check-budget: FAILED" >&2
else
  echo "check-budget: passed"
fi
exit "$status"
