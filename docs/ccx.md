# CalculiX (`ccx`) adapter

`fh-solve-ccx` runs CalculiX as a protocol v2 solver (D-13). femahoy never bundles or links `ccx`, because it is GPL. You install it.

## Get `ccx`

- Nix (macOS, Linux): `nix build nixpkgs#calculix-ccx -o ccx`, then `export FH_CCX=$PWD/ccx/bin/ccx`.
- Debian, Ubuntu: `apt install calculix-ccx`. This puts `ccx` on PATH.

The adapter uses `$FH_CCX`. If `FH_CCX` is not set, it uses `ccx` on PATH.

## Run

```
fh-solve-ccx model.inp run-dir                                # one run
FH_SOLVE=$PWD/target/release/fh-solve-ccx fh validate          # the validation suite through ccx
```

The run folder gets `results.pvd`, `step-N.vtu` and `summary.toon`, as with `fh-solve`. The `ccx` files (`job.inp`, `job.frd`, `job.log`) are in `run-dir/ccx`.
The summary says `solver: "ccx <version>, unvalidated"`.

## Keyword subset

`*NODE`, `*ELEMENT` (`C3D8`, `C3D8I`, `C3D10`), `*NSET`, `*ELSET`, `*SURFACE, TYPE=ELEMENT`, `*MATERIAL`, `*ELASTIC`, `*SOLID SECTION`, `*BOUNDARY`, `*STEP`, `*STATIC`, `*CLOAD`, `*DLOAD`, `*DSLOAD`, and `*PART`, `*ASSEMBLY`, `*INSTANCE` (flattened).
The adapter refuses `*EQUATION`, `*MPC`, `*COUPLING`, `*SURFACE, TYPE=NODE` and `NLGEOM` with a clear error.

Changes for `ccx`:

- A model with no `*SOLID SECTION` gets one section for all elements with its only material.
- Each step is one linear increment. The adapter ignores the `*STATIC` data line.
- The element stress `S` is the mean of the nodal stress that `ccx` writes. `fh-solve` uses the volume mean of the integration points.

## Limits

- The `.frd` file has 6 significant digits.
- `ccx` extrapolates the stress to the nodes with an error of about 1e-5 for a uniform field. The integration point stress in `job.dat` is exact.
- Thus class N cases with a tolerance of 1e-6 fail on stress. Class L and class R cases pass.

## Record of stage C1 (2026-10-18)

Tested `ccx`: 2.23 from `nixpkgs#calculix-ccx`, on macOS aarch64.
`FH_SOLVE=fh-solve-ccx fh validate`: 10 of 16 cases pass. All class L and class R cases pass. The 6 class N cases (`bar-pressure`, `bar-pressure-m`, `bar-tension`, and the 3 patch tests) fail on stress only, for the reason in Limits.

| Case | Quantity | `ccx` | `fh-solve` | Reference |
|---|---|---|---|---|
| `cantilever` | tip uz, Richardson limit (mm) | -1.9087822e-2 | -1.9087899e-2 | -1.919619e-2, pass |
| `cantilever` | tip uz, mesh-32 (mm) | -1.90587e-2 | -1.9058737e-2 | |
| `lame-cylinder` | ur inner (mm) | 9.0796256e-2 | 9.0794389e-2 | 9.0793651e-2, pass |
| `bar-pressure` | u_x tip (mm) | 4.76190e-2 | 4.7619048e-2 | 4.7619048e-2, pass |
| `bar-pressure` | rf_x (N) | -20000.01 | -20000.00 | -20000, pass |
| `bar-pressure` | s_xx (MPa) | 99.9990 | 100.0000 | 100, fail at tolerance 1e-4 MPa |
