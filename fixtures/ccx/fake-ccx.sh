#!/bin/sh
# A fake `ccx` for the tests: `fake-ccx.sh -i job` in the `ccx` work folder. It reads the
# elements and the steps of job.inp and writes job.frd and job.dat with zero results, in the
# format of ccx 2.23. Thus a run of any model succeeds with no CalculiX.
echo "CalculiX Version 0.0-fake, Copyright"
awk '
/^\*/ { element = ($0 ~ /^\*ELEMENT/); if ($0 ~ /^\*END STEP/) steps++; next }
element { sub(/,.*/, ""); ids[++n] = $0 }
END {
  for (s = 1; s <= steps; s++) {
    printf "    1PSTEP 1 1 %d\n  100CL  101 1.0\n -4  DISP\n", s > "job.frd"
    printf " -1%10d%12.5E%12.5E%12.5E\n -3\n", 1, 0, 0, 0 > "job.frd"
    print " stresses (elem, integ.pnt.,sxx,syy,szz,sxy,sxz,syz)" > "job.dat"
    for (i = 1; i <= n; i++) for (p = 1; p <= 4; p++) print ids[i], p, 0, 0, 0, 0, 0, 0 > "job.dat"
  }
  print " 9999" > "job.frd"
}' job.inp
