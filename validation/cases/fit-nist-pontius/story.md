## Conclusion

`fh fit` gives the certified values of NIST for the data set Pontius. The fit of y = c0 + c1 x + c2 x^2 to the 40 observations gives c0, c1, c2 and R-squared within 6.4e-13 of the certified values, relative. The largest difference is in c0, 0.6 % of the tolerance of 1e-10 (see "Tolerance").

<!--curve fit-nist-pontius-pontius-fit.csv-->

The plot shows the 40 observations of NIST and the fit of `fh fit poly2`. The largest residual is 4.4684e-4, at x = 300000. c0 has the largest relative difference because it is small: the other 2 terms of y are up to 3000 times larger.

The raw columns 1, x and x^2 differ in size by 10^13. Thus the solver first scales each column to the norm 1. This lowers the condition number of the matrix from 1.4e13 to 18.4. On the raw columns, `numpy.linalg.lstsq` gives c0 7.8e-8 from the reference, and the tolerance stops this error.
