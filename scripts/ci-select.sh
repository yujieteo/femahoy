#!/usr/bin/env bash
# Select the solvers that CI installs and the validation cases that it runs (DESIGN.md section 7).
#
#   git diff --name-only BASE HEAD | scripts/ci-select.sh
#   scripts/ci-select.sh --all
#
# The script reads the changed paths, 1 on each line, and writes lines for $GITHUB_OUTPUT:
# `<solver>=1` for each solver to install (ccx, openfoam, getdp, precice, chrono, lammps, meep),
# `all=1` when the full suite runs, else `cases=<id> ...`, the cases of the selected solvers.
# A changed case selects the solver of that case. A changed solver module selects its solver and
# each solver whose module uses it. A Markdown file or a file in docs/ selects no solver. Each
# other path (shared code, Cargo files, CI) selects every solver.
set -euo pipefail

every="ccx openfoam getdp precice chrono lammps meep"
selected=" "

add() {
  local s
  for s in "$@"; do
    case $selected in *" $s "*) ;; *) selected="$selected$s " ;; esac
  done
}

has() { [ -e "$1" ]; }

# The solver of the case folder $1, from the files of its input, or "all" when none matches.
# A fit case needs no solver. It runs with the ccx cases.
case_solver() {
  if [ -f "$1/job.inp" ] || has "$1"/*/topology.toml || has "$1"/*/fit.toml; then echo ccx
  elif has "$1"/*/precice-config.xml; then echo precice
  elif has "$1"/*/case.pro; then echo getdp
  elif has "$1"/*/system/controlDict; then echo openfoam
  elif has "$1"/*/chrono.py; then echo chrono
  elif has "$1"/*/case.lmp; then echo lammps
  elif has "$1"/*/meep_case.py; then echo meep
  else echo all
  fi
}

if [ "${1:-}" = --all ]; then
  add all
else
  while IFS= read -r path; do
    case $path in
      *.md | docs/*) ;;
      validation/cases/*/*)
        id=${path#validation/cases/}
        id=${id%%/*}
        if [ -d "validation/cases/$id" ]; then add "$(case_solver "validation/cases/$id")"; else add all; fi
        ;;
      crates/fh/src/chrono.rs) add chrono meep ;;
      crates/fh/src/lammps.rs) add lammps ;;
      crates/fh/src/meep.rs) add meep ;;
      crates/fh/src/precice.rs) add precice openfoam ;;
      crates/fh/src/topology.rs) add ccx ;;
      crates/fh/src/foam.rs) add openfoam precice ;;
      crates/fh/src/getdp.rs) add getdp chrono lammps meep precice ;;
      *) add all ;;
    esac
  done
fi

missing=0
for s in $every; do
  case $selected in *" $s "*) ;; *) missing=1 ;; esac
done
[ $missing = 1 ] || add all
case $selected in *" all "*) selected=" $every all " ;; esac

# The preCICE case also needs OpenFOAM installed, but not the OpenFOAM cases.
install=$selected
case $selected in *" precice "*) install="$install openfoam " ;; esac
for s in $every; do
  case $install in *" $s "*) echo "$s=1" ;; esac
done
case $selected in
  *" all "*) echo "all=1" ;;
  *)
    cases=""
    for dir in validation/cases/*/; do
      dir=${dir%/}
      case $selected in *" $(case_solver "$dir") "*) cases="$cases ${dir##*/}" ;; esac
    done
    echo "cases=${cases# }"
    ;;
esac
