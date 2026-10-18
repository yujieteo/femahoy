# Memory of solver phases (E1)

The memory gate solves a generated box with 1,000,000 C3D8 elements.
It requires a successful solve and a peak at most 16,000,000,000 bytes (decimal GB).
This limit is below 16 GiB.
The box has 100 elements on each axis, one fixed face, and one transverse point load on the opposite face.
The material has E = 1000 and nu = 0.3.
All 3 result formats and the default restart output are on.
The script does not change the solver physics or the validation fixtures.

Use a new scratch folder outside the repository:

```sh
cargo +stable build --release -p fh-core-solver --bin fh-solve
python3 scripts/check-phase-memory.py --solver target/release/fh-solve \
  --output /tmp/fh-memory-1m --threads 4
python3 scripts/test-phase-memory.py
```

The default `--n 100` gives 1M elements. A smaller `--n` is a smoke check, not evidence for the 1M target.
The script writes the input, solver output, sample CSV, kernel statistics, and `memory.toon` in the scratch folder.
It fails on a memory excess, a timeout, a failed solve, missing phase samples, or missing kernel statistics.
The timeout is 1800 s by default. Use `--timeout` for a slower machine.
The script currently requires macOS. It uses only the Python standard library and `/usr/bin/time`.

## Probe and phase boundaries

The macOS guard uses `proc_listpids(PROC_PGRP_ONLY)` to list the process group.
It sums `ri_phys_footprint` from `proc_pid_rusage(RUSAGE_INFO_V2)` for those processes.
It no longer starts `ps` every 100 ms.
The footprint includes compressed memory that RSS does not count.
The guard keeps the 100 ms interval.
An empty list or a probe error fails the guard unless the child exited.
A process that exits between the list and the query does not count.
The ABI comes from the macOS SDK headers `libproc.h`, `sys/proc_info.h`, and `sys/resource.h`.
Apple's [libproc source](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c) also specifies the 0 return on list errors.

The script samples the group footprint every 100 ms.
The group includes the `/usr/bin/time` parent, so the samples include its small footprint too.
The script also reads the solver's kernel peak footprint from `/usr/bin/time -l`.
The gate checks both peaks. Thus a short peak between samples still fails the memory gate.
Only the phase attribution uses samples, so a short peak can fall between phase samples.

The existing protocol defines these measurement boundaries:

| Phase | Work in the measurement interval |
|---|---|
| Read | Process start to `estimate`. Includes input text, parse, base grid, mesh, and initial node graph. |
| Assemble | `estimate` to the first `phase=solve`. Includes constraints, matrix pattern, slot map, and assembly. |
| Factor or iterate | `phase=solve` to `phase=recover`. Includes the factor or PCG preconditioner, solve, and residual check. |
| Recover | `phase=recover` to the `step` line. Includes reactions, stress, and result arrays. |
| Write | `step` to process exit. Includes text, VTU, VTK, restart, PVD, and summary output. |

Each value is the full live footprint during that interval, not an allocation delta.
The broad read and assembly intervals include setup because the protocol has no separate parse or pattern event.
No solver instrumentation or numerical code changed.

## Measured run

The run used macOS 27.0.1 (26A434), Apple M5, arm64, 10 cores, and 16 GiB RAM.
The release build used the default Rust target and 4 solver threads.
This run checks memory on macOS. It does not verify the E3 x86-64 CPU target or a run without swap.

The table below records the run from 2026-10-13.

| Phase | Peak footprint (bytes) | Peak footprint (MiB) |
|---|---:|---:|
| Read | 1,111,344,544 | 1059.9 |
| Assemble | 4,402,319,440 | 4198.4 |
| Factor or iterate | 6,006,560,000 | 5728.3 |
| Recover | 5,711,926,336 | 5447.3 |
| Write | 5,822,780,552 | 5553.0 |
| Whole solver, kernel peak | 6,005,445,648 | 5727.2 |

The largest group sample was 6.01 GB. The check passed against 16 GB.
The kernel maximum RSS was 3,276,341,248 bytes, much below the kernel peak footprint.
This run shows why RSS cannot enforce the macOS footprint limit.
The sample maximum exceeds the kernel peak by about 1.1 MiB because the group includes `/usr/bin/time`.

The run took 272.782 s. PCG took 948 iterations for 3,060,300 free dofs.
The script made 2675 native probes. Their total wall time was 0.529569 s.
The full monitor used 0.712602 s CPU, including the probes and the log files.
This is the script's CPU cost, not a separate benchmark of the Rust runner.

The recorded command was:

```sh
python3 scripts/check-phase-memory.py --solver target/release/fh-solve \
  --output /Users/yj/src/firstmate/data/femahoy-phase-memory-e1/box-1m --threads 4
```

The evidence files in that folder are `memory.toon`, `samples.csv`, `stdout.txt`, and `time.txt`.
The relevant output is:

```text
elements: 1000000
threads: 4
limit_bytes: 16000000000
kernel_peak_bytes: 6005445648
samples: 2675
probe_wall_s: 0.529569
monitor_cpu_s: 0.712602
wall_s: 272.782
status: pass
step 1 ... free_dofs=3060300 method=pcg iterations=948
```

## Regression evidence

Before the probe change, this command failed:

```sh
cargo +stable test -p fh-solver-api macos_probe_reads_footprint_instead_of_rss
```

```text
assertion `left == right` failed
  left: 1081344
 right: 1081584
test result: FAILED. 0 passed; 1 failed
```

After the change, the same test passed.
The test compares the probe with an independent SDK footprint query of an isolated `/bin/sleep` process.
A second test checks group members and rejects a group with no readable processes.
The script tests check the kernel peak, the sampled peak, missing phases, a failed solver, and the generated mesh.
A small fake solver also checks the complete monitor and its output files.

| Command | Result |
|---|---|
| `cargo +stable check -p fh-solver-api` | Pass |
| `cargo +stable test -p fh-solver-api` | 8 unit tests and 40 subprocess tests passed |
| `cargo +stable clippy -p fh-solver-api --all-targets -- -D warnings` | Pass |
| `cargo +stable fmt --check` | Pass |
| `python3 scripts/test-phase-memory.py` | 7 tests passed |
| `PATH=$HOME/.cargo/bin:$PATH bash scripts/check-budget.sh` | Pass. solver-api uses 2217 of 3000 lines. |

The solver-api crate grew from 2116 to 2217 lines.
No dependency changed. The full workspace gate and independent review follow the task handoff.

## E3: x86-64 without AVX under an emulator

The workflow `no-avx` (`.github/workflows/no-avx.yml`) runs `scripts/check-no-avx.sh` on a GitHub x86-64 runner with Ubuntu 24.04.
QEMU user mode (`qemu-x86_64 -cpu Nehalem`) runs each program with the Nehalem CPU model: SSE4.2 and no AVX.
An AVX instruction stops with SIGILL under this model.
The emulator is not a real CPU without AVX. All times and memory values in this section are emulator values, not values of the D-06 machine.
The 1-million-element repeat of M2.6 on x86-64 is still open. This record has small models only.

| Item | Value |
|---|---|
| Date | 2026-10-18 |
| Run | [39125169508](https://github.com/yujieteo/femahoy/actions/runs/39125169508). Job time 18 min 42 s. Step `Checks under QEMU Nehalem` 17 min 5 s. |
| Commit | `47f33ef`: the scripts and the test of this change, with 1 more line that adds the branch `fm/femahoy-e3-no-avx-x1-probe` to the push trigger |
| Runner CPU | AMD EPYC 9V74 80-Core Processor, 4 cores, 16,373,452 kB RAM. The host has AVX, AVX2 and AVX-512, and the emulator hides them. |
| Emulator | qemu-x86_64 8.2.2 (Debian 1:8.2.2+ds-0ubuntu1.18) |
| Tools | rustc 1.99.1, Python 3.12.15, build123d 0.13.0, cadquery-ocp-novtk 8.0.1.1.0, numpy 2.5.4, gmsh 4.15.2 (pip wheel) |
| Build | Release `fh` and `fh-solve` for `x86_64-unknown-linux-gnu` with `-C target-cpu=x86-64 -C target-feature=+sse4.2,-avx,-avx2` |

2 earlier runs of the same job passed on different runner CPUs:

| Run | Runner CPU | Job time | Time of the CI job `check` in the same push |
|---|---|---:|---:|
| [39123409918](https://github.com/yujieteo/femahoy/actions/runs/39123409918) | AMD EPYC 9V45 | 12 min 15 s | 6 min 0 s |
| [39124531969](https://github.com/yujieteo/femahoy/actions/runs/39124531969) | AMD EPYC 7763 | 21 min 32 s | 4 min 11 s |
| [39125169508](https://github.com/yujieteo/femahoy/actions/runs/39125169508) | AMD EPYC 9V74 | 18 min 42 s | 3 min 59 s |

The emulator time depends on the runner CPU.
Each result file of the thread check has the same SHA-256 in all 3 runs.
The first run had an older version of `scripts/check-threads.py`, so its memory values are not in this record.

### Time and peak memory

GNU time is the parent process of each step. The elapsed time is the wall clock time.
The peak is the maximum resident set size in MiB. Linux gives the largest process of the step, for example gmsh in `fh mesh`.
Each value includes the memory of QEMU itself.
The table gives the values of run 39125169508. A restart row of the thread check adds the times of its 2 processes and gives the larger peak.

| Step | Programs | Runs | Elapsed s (emulator) | Peak RSS MiB (emulator) |
|---|---|---:|---:|---:|
| `fh validate` on `validation/cases` | fh, fh-solve | 1 | 29.16 | 140.7 |
| CAD part, `scripts/cad/bar.py` | Python, build123d | 5 | 24.18 to 24.76 | 562.9 to 565.0 |
| `fh info` of the STEP file | fh | 5 | 0.04 | 20.1 to 20.2 |
| `fh mesh` | fh, gmsh | 5 | 2.68 to 3.17 | 142.3 to 143.3 |
| `fh info` of the mesh | fh | 5 | 0.05 to 0.09 | 20.4 to 21.7 |
| `fh model` | fh | 5 | 0.06 to 0.13 | 21.5 to 25.0 |
| `fh check` of the model | fh | 5 | 0.04 to 0.07 | 20.8 to 23.2 |
| Refused inputs of the M2b gate | fh, gmsh | 7 | 0.03 to 2.72 | 20.0 to 143.5 |
| `fh validate --root`, coarse meshes | fh, fh-solve | 1 | 1.12 | 26.5 |
| `fh validate --root`, fine meshes | fh, fh-solve | 1 | 5.53 | 56.5 |
| Thread check, 32 rows (table below) | fh-solve | 48 | 0.41 to 76.88 for each row | 24.3 to 159.9 |
| Matrix test (below) | test binary of `fh-core-solver` | 1 | 5.28 | 55.7 |

### Result bytes at 1, 2, 4 and 6 threads

`scripts/check-threads.py` runs `fh-solve` under the emulator with `FH_THREADS` set to 1, 2, 4 and 6.
The inputs are `bar-tension` and the fine `bar-pressure` model of the CLI workflow, and the C3D8 boxes of `scripts/check-phase-memory.py` with 14 and 28 elements on each axis.
Each input gets a second step with twice the loads.
A full run solves both steps. A restart run stops with SIGKILL after the first restart file, and then `--restart` solves step 2 in the same folder.
Each run folder has 10 files: `restart.fh`, `restart.fh.prev`, `results.pvd`, and `step-N.txt`, `step-N.vtu` and `step-N.vtk` for each step, and `summary.toon`.
These files hold the displacements, the residual forces RF (internal force minus load at each node), the stresses and the restart state.
Every file of the 32 run folders has the bytes of the full run at 1 thread. Only the summary lines `wall_s` and `run_id` can differ, and the check does not compare them.
The step lines of the full runs are also equal.

| Input | Method | Free dofs | Full run, elapsed s (emulator) at 1, 2, 4, 6 threads | Full run, peak RSS MiB (emulator) at 1, 2, 4, 6 threads | Same bytes |
|---|---|---:|---|---|---|
| `bar-tension` | Cholesky | 2071 | 0.50, 0.43, 0.41, 0.41 | 24.3, 26.3, 26.7, 27.3 | yes |
| `bar-pressure` | Cholesky | 8938 | 3.36, 2.98, 2.91, 2.92 | 44.5, 48.7, 56.4, 61.2 | yes |
| box, 14 elements on each axis | Cholesky | 9450 | 17.61, 16.93, 16.65, 16.65 | 82.0, 83.0, 86.0, 97.7 | yes |
| box, 28 elements on each axis | PCG, 275 iterations in each step | 70644 | 64.87, 35.05, 30.54, 31.26 | 142.8, 144.4, 154.8, 158.6 | yes |

The solver writes no file of the matrix. Thus the unit test `step::tests::the_matrix_has_the_same_bits_for_any_thread_count` in `crates/core-solver/src/step.rs` compares the matrix values.
The test assembles a C3D8 box with 16 elements on each axis (4096 elements) at 1, 2, 4 and 6 threads.
It compares the bits of the matrix values and of the internal forces.
In an assembly, only the element evaluations use threads. The main thread adds the results in element order.
The matrix check uses the test binary, not the release `fh-solve`. The test binary has the same target and RUSTFLAGS, and it ran under the same emulator in run 39125169508.
The test passed: `test result: ok. 1 passed`.

### CPU features of the external programs

All external programs ran under the Nehalem model, and no program stopped with SIGILL.
The script found the CPU features in 2 ways:

1. `numpy.show_runtime()` under the emulator. numpy is a dependency of build123d. It reports the baseline `X86_V2` (SSE3 to SSE4.2 and POPCNT), no extra feature found, and `X86_V3`, `X86_V4`, `AVX512_ICL` and `AVX512_SPR` not found. Its OpenBLAS selects the `Nehalem` kernels.
2. `readelf -n` reads the "x86 ISA needed" note of each ELF file: the Python program, `libgmsh.so.4.15` and each shared library of the Python packages (329 files).

| Program | ISA note | Result under Nehalem |
|---|---|---|
| Python 3.12.15 (`python3.12`) | x86-64-baseline | Ran |
| 5 libraries of `cadquery_ocp_novtk.libs` (Imath, Iex, IexMath, IlmThread, Half) | SSE, SSE2 | Ran |
| gmsh 4.15.2 (`libgmsh.so.4.15`) and the other 322 libraries | No ISA note | Ran |

A file with no ISA note does not declare its CPU needs. For those files, the run under the Nehalem model is the evidence.
These are the needs of the tested versions on this emulator. A different build of a program can need different features.

### Open

- The 1-million-element run of M2.6 on x86-64 without AVX.
- A run on a real CPU without AVX. The emulator gives no hardware times.
