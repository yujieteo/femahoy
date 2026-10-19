# CalculiX (`ccx`) adapter

`fh` starts CalculiX directly as a separate program (D-13). The crate `fh-solver-ccx` has the adapter code. femahoy never bundles or links `ccx`, because it is GPL. You install it.

`ccx` is the only solver of femahoy. The owner removed the native solver `fh-solve` on 2026-10-19 (D-29). An own solver is a main advantage later, but not now. Main commit `08a97c2` has the native solver.

## Get `ccx`

- Nix (macOS, Linux): `nix build nixpkgs#calculix-ccx -o ccx`, then `export FH_CCX=$PWD/ccx/bin/ccx`.
- Debian, Ubuntu: `apt install calculix-ccx`. This puts `ccx` on PATH.

The adapter uses `$FH_CCX`. If `FH_CCX` is not set, it uses `ccx` on PATH.
If `ccx` does not start, each solve and `fh validate` stop with an error that refers to this page.

## Run

```
fh run model.inp    # one run through the daemon
fh validate         # the validation suite through ccx
```

The run folder gets `results.pvd` and `step-N.vtu`. There is no summary file: the run record has the state and the time. The `ccx` files (`job.inp`, `job.frd`, `job.dat`, `job.log`) are in `run-dir/ccx`.
U and RF come from `job.frd`. The stress comes from the integration points in `job.dat` (`*EL PRINT`).

## Run folder

- A run clears the run folder before it starts, but only if the folder has the marker file `.fh-run`.
- If the folder is not empty and has no marker, the run stops with an error and deletes nothing.
- The run writes the marker on the first run.
- Each result file gets a temporary name first, and then a rename (D-09).

## Result contract

`fh validate` reads these arrays in each VTU file. The adapter uses the shared code in `fh_io::result` for the ids, the set flags and the rule for `S`.

- `NODE_ID` on the points and `ELEMENT_ID` on the cells, in model order.
- `NSET_<name>` on the points and `ELSET_<name>` on the cells, one for each set of the model. The value is 1 for a member of the set and 0 for all other entries.
- `U` and `RF` on the points.
- `S` on the cells, in the order `[xx, yy, zz, xy, yz, xz]`. `S` is the volume mean of the integration point stress. The weight of each point is its integration weight times its Jacobian determinant.
- `MISES` on the cells, from `S`.

## Process limits

- There is no default time limit, because a large model can solve for hours. A cancel stops a run.
- The default memory limit is 85% of the machine RAM. If the RAM is not known, there is no limit.
- A validation case can set its own limits with `time_limit_s` and `memory_limit_mib`.
- The error at a limit tells the user to use a coarser mesh.
- `ccx` runs in its own process group. A cancel, the time limit and the memory limit kill the whole group at once.
- A guard process in the group reads a pipe from `fh`. If `fh` stops, also on SIGKILL, the pipe closes and the guard kills the group. Thus `ccx` never runs longer than `fh`.
- The memory check sums the memory of the whole group (Linux: the resident memory from `/proc`, macOS: the physical footprint).
- Linux: the run also sets a hard address space limit (limit + 256 MiB).
- macOS has no hard limit that the kernel enforces. Only the memory check applies.
- If the memory check fails, the run stops `ccx` with an error.

## Keyword subset

`*NODE`, `*ELEMENT` (`C3D10` only), `*NSET`, `*ELSET`, `*SURFACE, TYPE=ELEMENT`, `*MATERIAL`, `*ELASTIC`, `*EXPANSION`, `*SOLID SECTION`, `*INITIAL CONDITIONS, TYPE=TEMPERATURE`, `*BOUNDARY`, `*STEP`, `*STATIC`, `*CLOAD`, `*DLOAD`, `*DSLOAD` and `*TEMPERATURE`.
The parser refuses each other keyword, parameter and element type with a clear error ([inp.md](reference/inp.md)).

Changes for `ccx`:

- A model with no `*SOLID SECTION` gets one section for all elements with its only material.
- Each step is one linear increment. The parser does not read the `*STATIC` data lines.
- The element stress `S` is the volume mean of the `ccx` integration point stress ("Result contract" above).
- Each number in `job.inp` has 20 characters or fewer, because `ccx` reads only the first 20 characters of a number.
  A longer number lost the end of its exponent with no error. For example, `-1.33027879758290e-17` became -0.133.
- Before `ccx` starts, the adapter checks the supports of each step. They must hold the 6 rigid motions of each connected part of the mesh.
  `ccx` solves a model that can move as a rigid body with no error, and its result is wrong.
  Thus the adapter stops with an error that names a node of the part and the free motion.

## Errors

Each failure of `ccx` gives a message that tells why and what to do:

| Condition | Message |
|---|---|
| `job.log` has `*ERROR` | `ccx failed (STATUS): TEXT. Correct the model as the message tells.` `TEXT` is the full first `*ERROR` message, with its next lines. |
| A signal stops `ccx` | `ccx stopped on signal N and gave no error message. Usually the cause is too little memory for the model. Use a coarser mesh, or a computer with more memory.` |
| Any other exit code | `ccx failed (STATUS) with no *ERROR message. The last ccx line is: LINE.` |

Each message ends with the path of `job.log`.
The time limit and the memory limit give their own errors ("Process limits" above).

## Thermal strain

The adapter writes these keywords to `job.inp` with the same data, so `ccx` gives them the CalculiX meaning:

- `*EXPANSION`: the thermal strain is alpha (T - T0), with T0 the initial temperature of the node.
- `*INITIAL CONDITIONS, TYPE=TEMPERATURE`: the start temperature. Give a value to each node. Else `ccx` stops with `no initial temperature`.
- `*TEMPERATURE`: the temperature at the end of the step. A node with no value in a step keeps its last temperature.

A model with several materials needs one `*SOLID SECTION` for each element set.
`fh validate` runs each case with `ccx`. If `ccx` does not start, `fh validate` stops with exit code 1 before the first case. A run where no case ran does not pass.
If `ccx` does not start in `fh run`, the run fails with an error that refers to this page.

## Limits

- The `.frd` file has 6 significant digits, so U and RF have 6 digits. The `.dat` stress has 7.
- Thus a displacement with a small tolerance can fail on rounding. `bar-pressure` u_x tip passes with a margin of only 3e-17 mm.
- `ccx` extrapolates the stress to the nodes in `.frd` with an error of about 1e-5. Thus the adapter does not use the `.frd` stress.

## Record of the case cut (2026-10-19)

Tested `ccx`: 2.23, on macOS aarch64.
The owner cut the suite to 9 cases with 1 `C3D10` mesh each. `fh validate` runs each case through `ccx`: 9 of 9 cases pass.
The earlier records of this file, with the deleted cases, are in the Git history.

## Public warpage workflow

`validation/cases/warpage-strip` uses build123d bodies, STEP, gmsh physical volume groups, `fh model` and `fh run`.
The case has 2 materials, thermal expansion, and a uniform temperature change of 100 K.
The reference is the published bilayer solution of Lubarda and Lubarda, equations 2.5 to 2.7.
The [derivation](../validation/cases/warpage-strip/derivation.md) gives the source, inputs and measured results at 2 mesh sizes.
The fine mesh gives -0.6126606 mm against -0.6136986301 mm, a difference of 0.169143% within the 1% tolerance.
`fh validate` also checks the committed coarse mesh.

## Measured warpage use case

`validation/cases/warpage-thick-copper` compares the workflow with a measured warpage of a real part.
The part is a 50 x 10 x 0.75 mm slice of a power semiconductor wafer, with 730 µm of silicon and 20 µm of ECD copper.
The source is Calabretta et al., Applied Sciences 11 (2021) 5140, open access under CC BY 4.0.
Phase-shift moiré gives 0.700 µm/°C from 50 to 90 °C, in the second cycle, where the warpage is linear.
The case uses the materials of the paper and a temperature change of 40 K.
The [derivation](../validation/cases/warpage-thick-copper/derivation.md) gives the inputs, the 12% tolerance and the results at 3 mesh sizes.

The finest mesh gives 0.67738 µm/K, 3.2% below the measured value.
The bilayer plate theory with the same inputs gives 0.67813 µm/K. Thus the solve adds almost no error. The difference comes from the inputs, the model assumptions and the measurement.
The main causes are the low silicon modulus of the paper, the thin layers that the model leaves out, and the moiré accuracy.

## Plate on its edge (2026-10-19)

`validation/cases/plate-on-edge` is the steel plate of a chat: 3756 x 1000 x 2.4 mm, on its edge, with 2 fixed ends, 2 pins and 4 loads.
The own solver did not solve it. The first solve reached the time limit, and the second solve did not converge.
The case uses the 20 mm C3D10 mesh of the chat, without the back-face support that the third solve added.
It has `time_limit_s = 120` and `memory_limit_mib = 6144`.

The reference comes from a separate graded `C3D8I` model at 3 levels (`reference.py`), with Richardson extrapolation. This model is not a case.
The [derivation](../validation/cases/plate-on-edge/derivation.md) gives the model, the reference and the mesh study.
On a laptop with 16 GB of memory, `ccx` 2.23 gives these values:

| Mesh | Time (s) | Memory (GiB) | Right pin (N) | Left pin (N) |
|---|---:|---:|---:|---:|
| 20 mm, 139395 nodes | 12.0 | 2.91 | 2355.62 | -1322.91 |
| 10 mm, 527176 nodes | 77.5 | 7.50 | 2321.01 | -1299.85 |
| Reference | | | 2279.2 | -1274.6 |

The 20 mm mesh is 3.4% and 3.8% from the reference. The case tolerance is 5%.
