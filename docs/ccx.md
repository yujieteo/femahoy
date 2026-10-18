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

The run folder gets `results.pvd`, `step-N.vtu` and `summary.toon`, as with `fh-solve`. The `ccx` files (`job.inp`, `job.frd`, `job.dat`, `job.log`) are in `run-dir/ccx`.
U and RF come from `job.frd`. The stress comes from the integration points in `job.dat` (`*EL PRINT`).
The summary says `solver: "ccx <version>, unvalidated"`.

## Keyword subset

`*NODE`, `*ELEMENT` (`C3D8`, `C3D8I`, `C3D10`), `*NSET`, `*ELSET`, `*SURFACE, TYPE=ELEMENT`, `*MATERIAL`, `*ELASTIC`, `*EXPANSION`, `*SOLID SECTION`, `*INITIAL CONDITIONS, TYPE=TEMPERATURE`, `*BOUNDARY`, `*STEP`, `*STATIC`, `*CLOAD`, `*DLOAD`, `*DSLOAD`, `*TEMPERATURE`, and `*PART`, `*ASSEMBLY`, `*INSTANCE` (flattened).
The adapter refuses `*EQUATION`, `*MPC`, `*COUPLING`, `*SURFACE, TYPE=NODE` and `NLGEOM` with a clear error.

Changes for `ccx`:

- A model with no `*SOLID SECTION` gets one section for all elements with its only material.
- Each step is one linear increment. The adapter ignores the `*STATIC` data line.
- The element stress `S` is the volume mean of the `ccx` integration point stress, as in `fh-solve` (`docs/solver-protocol.md`, "Result contract").

## Thermal strain

Only the adapter solves thermal strain. `fh-solve` refuses `*EXPANSION`, `*INITIAL CONDITIONS, TYPE=TEMPERATURE` and `*TEMPERATURE` with an error that names the keyword.
The adapter writes these keywords to `job.inp` with the same data, so `ccx` gives them the CalculiX meaning:

- `*EXPANSION`: the thermal strain is alpha (T - T0), with T0 the initial temperature of the node.
- `*INITIAL CONDITIONS, TYPE=TEMPERATURE`: the start temperature. Give a value to each node. Else `ccx` stops with `no initial temperature`.
- `*TEMPERATURE`: the temperature at the end of the step. A node with no value in a step keeps its last temperature.

A model with several materials needs one `*SOLID SECTION` for each element set.
The case `validation/cases/bimetal-strip` has the key `solver = "fh-solve-ccx"`, so `fh validate` runs it with `fh-solve-ccx`.

## Limits

- The `.frd` file has 6 significant digits, so U and RF have 6 digits. The `.dat` stress has 7.
- Thus a class N displacement can fail on rounding. `bar-pressure` u_x tip passes with a margin of only 3e-17 mm.
- `ccx` extrapolates the stress to the nodes in `.frd` with an error of about 1e-5. Thus the adapter does not use the `.frd` stress.

## Record of stage C1 (2026-10-18)

Tested `ccx`: 2.23 from `nixpkgs#calculix-ccx`, on macOS aarch64.
`FH_SOLVE=fh-solve-ccx fh validate`: 16 of 16 cases pass. The tolerances are the same as for `fh-solve`.

| Case | Quantity | `ccx` | `fh-solve` | Reference |
|---|---|---|---|---|
| `cantilever` | tip uz, Richardson limit (mm) | -1.9087822e-2 | -1.9087899e-2 | -1.919619e-2, pass |
| `cantilever` | tip uz, mesh-32 (mm) | -1.90587e-2 | -1.9058737e-2 | |
| `lame-cylinder` | ur inner (mm) | 9.0796256e-2 | 9.0794389e-2 | 9.0793651e-2, pass |
| `bar-pressure` | u_x tip (mm) | 4.76190e-2 | 4.7619048e-2 | 4.7619048e-2, pass |
| `bar-pressure` | rf_x (N) | -20000.01 | -20000.00 | -20000, pass |
| `bar-pressure` | s_xx (MPa) | 100.0000 | 100.0000 | 100, pass |
| `patch-test` | s_xx (MPa) | 327.6000 | 327.6000 | 327.6, pass |

## Record of the thermal strain change (2026-10-18)

Tested `ccx`: 2.23 from `nixpkgs#calculix-ccx`, on macOS aarch64.
`fh validate` and `FH_SOLVE=fh-solve-ccx fh validate`: 17 of 17 cases pass. The 16 earlier cases give the same numbers as before.

| Case | Quantity | `ccx` | Reference |
|---|---|---|---|
| `bimetal-strip` | u3 at `P20` (mm) | -1.53425e-1 | -1.5342466e-1, pass |
| `bimetal-strip` | u1 at `P20` (mm) | 1.80548e-2 | 1.8054795e-2, pass |
