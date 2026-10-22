# fit-nist-pontius derivation

This case checks `fh fit`: the least squares fit of a model to a curve. The reference is a published fit with certified values. Thus the tolerance comes only from the rounding of the reference and from the round-off of the fit.

## Why this case

No other case of the suite checks a fit. `fh fit` gives the parameters that the agent puts in a model, for example the constants of `*HYPERELASTIC`. A wrong fit gives a wrong model with no warning.

Each model of `fh fit` is linear in its parameters, or in ln x and ln y (`power`). Thus all models use the same least squares solver, and this case checks that solver. The rows of the models are in `crates/fh/src/fit.rs`. Its unit tests check that each model gives its parameters back from exact points, and that `mooney-rivlin` gives the nominal stress of a C3D8 element of ccx 2.23 with the same constants.

## Source

NIST, Statistical Reference Datasets (StRD), linear least squares regression, data set Pontius: P. Pontius, NIST, load cell calibration. https://www.itl.nist.gov/div898/strd/lls/data/Pontius.shtml. The data and the certified values are in the file `Pontius.dat` of that page. Works of NIST are not under copyright in the United States.

The data are 40 observations: the deflection y of a load cell at the loads x = 150000 to 3000000, 2 at each load. The source gives no units. The model of the source is y = B0 + B1 x + B2 x^2. NIST computed the certified values in 500-digit arithmetic and gives them to 15 significant digits. NIST rates the data set "lower level of difficulty".

| Check | Parameter | Certified value |
|---|---|---:|
| `c0` | B0 | 0.673565789473684E-03 |
| `c1` | B1 | 0.732059160401003E-06 |
| `c2` | B2 | -0.316081871345029E-14 |
| `r2` | R-squared | 0.999999900178537 |

## Model

The folder `pontius` holds `pontius.csv`, the curve with the 40 observations as the series `NIST`, and `fit.toml`, which names the curve and the model `poly2`. `fh validate` fits the curve and writes the table `fit.txt` with the columns `c0`, `c1`, `c2` and `r2`. The command `fh fit poly2 pontius/pontius.csv` prints the same fit.

The solver is Householder QR on the columns 1, x and x^2, each first scaled to the norm 1. The raw columns differ in size by 10^13, and the condition number of the matrix is 1.4e13. After the scaling it is 18.4.

## Tolerance

The tolerance is `rel_tol = 1e-10` and `abs_tol = 0`.

| Cause | Basis | Effect |
|---|---|---:|
| Rounding of the reference | 15 significant digits | 5e-15 |
| Round-off of the fit | The largest relative difference of the 4 checks on this machine (`c0`), times 100 for another processor or compiler | 6.4e-11 |

`rel_tol` is the sum, 6.4e-11, rounded up to 1e-10.
The fit gives these relative differences to the certified values on a Mac (Apple Silicon) with Rust 1.97:

| Check | Relative difference | Part of the tolerance |
|---|---:|---:|
| `c0` | 6.4e-13 | 0.6 % |
| `c1` | less than 1e-15 | 0 % |
| `c2` | 4.1e-14 | 0.04 % |
| `r2` | 1e-16 | 0 % |

`c0` has the largest difference because it is small: the other 2 terms of y are up to 3000 times larger.

The tolerance stops these errors. The values come from NumPy 2.5 on the same data:

- No scaling of the columns: `numpy.linalg.lstsq` on the raw columns gives `c0` 7.8e-8 from the reference.
- 32-bit numbers: `c0` 1.8e-5 from the reference.
- 1 observation less (the first): `c0` 9 % from the reference.

The tolerance does not stop the normal equations in 64-bit numbers: they give `c0` 3.5e-12 from the reference. The data set has a low difficulty, so the normal equations are not wrong for it.

## Not in the check

- The other models of `fh fit`. They use the same solver, and the unit tests check their rows.
- The data sets of StRD with a higher difficulty, for example Filip (degree 10). They need more than 64-bit numbers.
