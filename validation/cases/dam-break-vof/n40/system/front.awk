# Read the table of the function object `front` (fieldExtents) of controlDict and write the
# dimensionless times T = t sqrt(2 g / a) at which the front is at Z = x / a = 2.11, 3.00 and
# 3.89. As in the source, a constant is added to T so that T = 1.19 at Z = 1.44 (derivation.md).
#
# The front is the largest x of a cell centre with alpha.water > 0.5. It moves 1 cell at a time.
# Thus the time at Z is the linear interpolation between the 2 steps of the front on each side
# of x = Z a.
BEGIN { a = 0.05715; g = 9.81; n = split("1.44 2.11 3.00 3.89", Z, " "); k = 1 }
/^#/ { next }
{
    t = $1; x = substr($5, 2) + 0
    if (!started) { t0 = t; x0 = x; started = 1 }
    if (x > x0) {
        for (; k <= n && x >= Z[k] * a; k++) tz[k] = t0 + (t - t0) * (Z[k] * a - x0) / (x - x0)
        t0 = t; x0 = x
    }
}
END {
    if (k <= n) { printf "front.awk: the front does not get to Z = %s\n", Z[k] > "/dev/stderr"; exit 1 }
    s = sqrt(2 * g / a)
    print "T_211 T_300 T_389"
    printf "%.4f %.4f %.4f\n", 1.19 + (tz[2] - tz[1]) * s, 1.19 + (tz[3] - tz[1]) * s, 1.19 + (tz[4] - tz[1]) * s
}
