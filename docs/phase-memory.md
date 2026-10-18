# Memory of solver phases (E1)

The memory gate solves a generated box with 1,000,000 C3D8 elements.
It requires a successful solve and a peak at most 16,000,000,000 bytes (decimal GB).
This limit is below 16 GiB.
The box has 100 elements on each axis, one fixed face, and one transverse point load on the opposite face.
The material has E = 1000 and nu = 0.3.
Only the default result output (VTU with PVD) and the default restart output are on.
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

No dependency changed. The full workspace gate and independent review follow the task handoff.
