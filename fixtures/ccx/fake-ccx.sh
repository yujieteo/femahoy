#!/bin/sh
# A fake `ccx` for the tests: `fake-ccx.sh -i job` in the `ccx` work folder. It reads the
# nodes, the elements (C3D4, C3D6, C3D8, C3D10, C3D15, C3D20) and the steps of job.inp and its
# `*INCLUDE` files. It writes job.frd and job.dat with zero results, in the format of ccx 2.23.
# Thus a run of any such model succeeds with no CalculiX.
echo "CalculiX Version 0.0-fake, Copyright"
awk '
function scan(file,    line) {
  while ((getline line < file) > 0) handle(line)
  close(file)
}
function finish_element(   parts, k, count) {
  count = split(pending, parts, ",")
  if (code[etype] == 0 || count < 2) return
  ne++
  eid[ne] = parts[1] + 0
  ecode[ne] = code[etype]
  enodes[ne] = count - 1
  for (k = 2; k <= count; k++) elist[ne, k - 1] = parts[k] + 0
}
function handle(line,    key, name, at) {
  sub(/\r$/, "", line)
  gsub(/ /, "", line)
  if (line ~ /^\*\*/ || line == "") return
  if (line ~ /^\*/) {
    if (mode == "element" && pending != "") { finish_element(); pending = "" }
    key = toupper(line)
    mode = ""
    if (key ~ /^\*ENDSTEP/) steps++
    else if (key ~ /^\*NODE(,|$)/) mode = "node"
    else if (key ~ /^\*ELEMENT(,|$)/) {
      mode = "element"
      if (match(key, /TYPE=[A-Z0-9]+/)) etype = substr(key, RSTART + 5, RLENGTH - 5)
      else etype = ""
    } else if (key ~ /^\*INCLUDE/) {
      name = line
      sub(/^.*[Ii][Nn][Pp][Uu][Tt]=/, "", name)
      sub(/,.*$/, "", name)
      scan(name)
    }
    return
  }
  if (mode == "node") {
    split(line, parts, ",")
    nn++
    nid[nn] = parts[1] + 0
    nx[nn] = parts[2] + 0; ny[nn] = parts[3] + 0; nz[nn] = parts[4] + 0
  } else if (mode == "element") {
    pending = pending line
    if (line !~ /,$/) { finish_element(); pending = "" }
  }
}
BEGIN {
  code["C3D8"] = 1; code["C3D6"] = 2; code["C3D4"] = 3
  code["C3D20"] = 4; code["C3D15"] = 5; code["C3D10"] = 6
  scan("job.inp")
  if (mode == "element" && pending != "") finish_element()
  printf "    2C%25d%37d\n", nn, 1 > "job.frd"
  for (i = 1; i <= nn; i++)
    printf " -1%10d%12.5E%12.5E%12.5E\n", nid[i], nx[i], ny[i], nz[i] > "job.frd"
  print " -3" > "job.frd"
  printf "    3C%25d%37d\n", ne, 1 > "job.frd"
  for (e = 1; e <= ne; e++) {
    printf " -1%10d%5d%5d%5d\n", eid[e], ecode[e], 0, 1 > "job.frd"
    for (k = 1; k <= enodes[e]; k++) {
      if (k % 10 == 1) printf " -2" > "job.frd"
      printf "%10d", elist[e, k] > "job.frd"
      if (k % 10 == 0 || k == enodes[e]) printf "\n" > "job.frd"
    }
  }
  print " -3" > "job.frd"
  for (s = 1; s <= steps; s++) {
    printf "    1PSTEP%25d%12d%12d\n  100CL  101%12.5E\n", s, 1, s, 1.0 > "job.frd"
    print " -4  DISP        4    1" > "job.frd"
    print " -5  D1          1    2    1    0" > "job.frd"
    print " -5  D2          1    2    2    0" > "job.frd"
    print " -5  D3          1    2    3    0" > "job.frd"
    for (i = 1; i <= nn; i++)
      printf " -1%10d%12.5E%12.5E%12.5E\n", nid[i], 0, 0, 0 > "job.frd"
    print " -3" > "job.frd"
    print " stresses (elem, integ.pnt.,sxx,syy,szz,sxy,sxz,syz)" > "job.dat"
    for (e = 1; e <= ne; e++) for (p = 1; p <= 4; p++) print eid[e], p, 0, 0, 0, 0, 0, 0 > "job.dat"
  }
  print " 9999" > "job.frd"
}'
