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
fh run job.inp      # one run through the daemon: CalculiX reads job.inp unchanged
fh validate         # the validation suite through ccx
```

The chat writes `job.inp` and gives it to CalculiX with each file that it includes (`*INCLUDE`). The adapter copies them into `run-dir/ccx` and starts `ccx -i job` there, so an include path is relative to the folder of `job.inp`. It does not change the text.
The run folder gets `results.pvd` and `step-N.vtu`. There is no summary file: the run record has the state and the time. The `ccx` files (`job.inp`, `job.frd`, `job.dat`, `job.sta`, `job.log`) are in `run-dir/ccx`.
The text results are in `job.dat` and `job.sta`. The input asks for them with `*NODE PRINT` and `*EL PRINT`. The page shows the fields of `job.frd`, which the input asks for with `*NODE FILE` and `*EL FILE`.

## Run folder

- A run clears the run folder before it starts, but only if the folder has the marker file `.fh-run`.
- If the folder is not empty and has no marker, the run stops with an error and deletes nothing.
- The run writes the marker on the first run.
- Each result file gets a temporary name first, and then a rename (D-09).

## Result contract

The adapter converts each nodal field in `job.frd` to a point array in `step-N.vtu`, for the last increment of each step. A frequency step gives one frame for each mode.

- `DISP` is `U`, `FORC` is `RF`, `STRESS` is `S` (order `[xx, yy, zz, xy, yz, xz]`), `TOSTRAIN` is `E` and `PE` is `PEEQ`. Each other block keeps its `ccx` name.
- `MISES` comes from `S`.
- A shell or a beam is a solid element in `job.frd` (code 1 to 6), so the page draws each element type that `ccx` writes.
- An input with no `*NODE FILE` and no `*EL FILE` gives no frame and no field. `fh run` then says that the page has no field to show.

## Process limits

- There is no default time limit, because a large model can solve for hours. A cancel stops a run.
- The default memory limit is 85% of the machine RAM. If the RAM is not known, there is no limit.
- `fh run --timeout SECONDS --memory MIB` sets the limits of one run. `fh validate` uses a time limit of 300 s.
- The error at a limit tells the user to use a coarser mesh.
- `ccx` runs in its own process group. A cancel, the time limit and the memory limit kill the whole group at once.
- A guard process in the group reads a pipe from `fh`. If `fh` stops, also on SIGKILL, the pipe closes and the guard kills the group. Thus `ccx` never runs longer than `fh`.
- The memory check sums the memory of the whole group (Linux: the resident memory from `/proc`, macOS: the physical footprint).
- Linux: the run also sets a hard address space limit (limit + 256 MiB).
- macOS has no hard limit that the kernel enforces. Only the memory check applies.
- If the memory check fails, the run stops `ccx` with an error.

## Input

CalculiX reads and checks the input. femahoy does not limit the keywords, the elements or the solver of `*STATIC, SOLVER=...`. A small reader takes the mesh, the sets, `*SURFACE`, `*BOUNDARY`, `*CLOAD`, `*DLOAD` and `*DSLOAD` for the preview and skips each other keyword ([inp.md](reference/inp.md)).

- `ccx` reads only the first 20 characters of a number. A longer number loses the end of its exponent with no error. For example, `-1.33027879758290e-17` became -0.133. `fh mesh` writes short numbers.
- `ccx` solves a model that can move as a rigid body with no error, and its result is wrong. The input must hold the supports.
- `fh mesh` writes `** units: SI-mm` (or `SI-m`) as the first comment. The run copies that unit into `results.pvd`, or writes `unspecified` when the line is missing.

## Errors

Each failure of `ccx` gives a message that tells why and what to do:

| Condition | Message |
|---|---|
| `job.log` has `*ERROR` | `ccx failed (STATUS): TEXT. Correct the model as the message tells.` `TEXT` is the full first `*ERROR` message, with its next lines. |
| A signal stops `ccx` | `ccx stopped on signal N and gave no error message. Usually the cause is too little memory for the model. Use a coarser mesh, or a computer with more memory.` |
| Any other exit code | `ccx failed (STATUS) with no *ERROR message. The last ccx line is: LINE.` |

Each message ends with the path of `job.log`.
The time limit and the memory limit give their own errors ("Process limits" above).

If `ccx` does not start, `fh validate` stops with exit code 1 before the first case. A run where no case ran does not pass.
If `ccx` does not start in `fh run`, the run fails with an error that refers to this page.

## Limits

- The `.frd` file has 6 significant digits, so U and RF have 6 digits. The `.dat` file has 7.
- Thus a displacement with a small tolerance can fail on rounding. `bar-pressure` u_x tip passes with a margin of only 3e-17 mm.
- `ccx` extrapolates the stress to the nodes in `.frd` with an error of about 1e-5. A check that needs the stress of the integration points uses `*EL PRINT` and `job.dat`.

## Record of the case cut (2026-10-19)

Tested `ccx`: 2.23, on macOS aarch64.
The owner cut the suite to 9 cases with 1 `C3D10` mesh each. `fh validate` runs each case through `ccx`: 9 of 9 cases pass.
The earlier records of this file, with the deleted cases, are in the Git history.

## Public warpage workflow

`validation/cases/warpage-strip` uses build123d bodies, STEP, `fh mesh --volume` boxes and `fh run`.
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
