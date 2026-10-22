# Read the table of the function object `front` (fieldExtents) of controlDict and write the
# dimensionless times T = t sqrt(2 g / a) at which the front is at Z = x / a = 2.11, 3.00 and
# 3.89. As in the source, a constant is added to T so that T = 1.19 at Z = 1.44 (derivation.md).
#
# The front is the largest x of a cell centre with alpha.water > 0.5. It moves 1 cell at a time.
# Thus the time at Z is the linear interpolation between the 2 steps of the front on each side
# of x = Z a.
#
# It also writes the curve front.csv for `fh report --curve`: Z at each step of the front, with
# the measured points of the source.
BEGIN { a = 0.05715; g = 9.81; n = split("1.44 2.11 3.00 3.89", Z, " "); k = 1 }
/^#/ { next }
{
    t = $1; x = substr($5, 2) + 0
    if (!started) { t0 = t; x0 = x; started = 1; m = 1; ts[1] = t; xs[1] = x }
    if (x > x0) {
        for (; k <= n && x >= Z[k] * a; k++) tz[k] = t0 + (t - t0) * (Z[k] * a - x0) / (x - x0)
        t0 = t; x0 = x
        m++; ts[m] = t; xs[m] = x
    }
}
END {
    if (k <= n) { printf "front.awk: the front does not get to Z = %s\n", Z[k] > "/dev/stderr"; exit 1 }
    s = sqrt(2 * g / a)
    print "T_211 T_300 T_389"
    printf "%.4f %.4f %.4f\n", 1.19 + (tz[2] - tz[1]) * s, 1.19 + (tz[3] - tz[1]) * s, 1.19 + (tz[4] - tz[1]) * s
    f = "front.csv"
    print "# title: Dam break, the surge front (interFoam, 40 cells across a)" > f
    print "# y: Z = x / a, the front [1]" > f
    print "# points measured: Martin and Moyce (1952), Table 2, the mean of the runs with a = 2.25 in" > f
    print "# note: The front is the largest x of a cell centre with alpha.water > 0.5. T is 1.19 at Z = 1.44, as in the source." > f
    print "T = t sqrt(2 g / a) [1],interFoam,measured" > f
    for (i = 1; i <= m; i++) printf "%.4f,%.4f,\n", 1.19 + (ts[i] - tz[1]) * s, xs[i] / a > f
    split("1.19 1.83 2.51 3.11", T, " ")
    for (j = 1; j <= n; j++) printf "%s,,%s\n", T[j], Z[j] > f
}
