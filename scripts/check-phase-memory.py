#!/usr/bin/env python3
"""Solve a generated C3D8 box and require a peak at or below 16 GB."""

import argparse
import ctypes
import errno
import os
from pathlib import Path
import re
import resource
import selectors
import signal
import subprocess
import sys
import time

LIMIT = 16_000_000_000  # Decimal GB, stricter than 16 GiB.
PHASES = ("read", "assemble", "iterate", "recover", "write")


def box(path, n):
    def node(x, y, z):
        return 1 + x + (n + 1) * (y + (n + 1) * z)

    with path.open("w") as out:
        out.write("*NODE\n")
        for z in range(n + 1):
            for y in range(n + 1):
                for x in range(n + 1):
                    out.write(f"{node(x, y, z)},{x},{y},{z}\n")
        out.write("*ELEMENT, TYPE=C3D8\n")
        element = 0
        for z in range(n):
            for y in range(n):
                for x in range(n):
                    element += 1
                    ids = [node(x + dx, y + dy, z + dz) for dx, dy, dz in
                           ((0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0),
                            (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1))]
                    out.write(",".join(map(str, [element, *ids])) + "\n")
        out.write(f"*ELSET, ELSET=ALL, GENERATE\n1,{element},1\n"
                  "*MATERIAL, NAME=M\n*ELASTIC\n1000,0.3\n"
                  "*SOLID SECTION, ELSET=ALL, MATERIAL=M\n"
                  "*STEP\n*STATIC\n*BOUNDARY\n")
        for z in range(n + 1):
            for y in range(n + 1):
                out.write(f"{node(0, y, z)},1,3\n")
        out.write(f"*CLOAD\n{node(n, n // 2, n // 2)},3,-1\n*END STEP\n")


def native_probe():
    if sys.platform != "darwin":
        raise ValueError("this footprint check requires macOS")
    lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    lib.proc_listpids.argtypes = [ctypes.c_uint32, ctypes.c_uint32,
                                 ctypes.c_void_p, ctypes.c_int]
    lib.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    lib.proc_listpids.restype = lib.proc_pid_rusage.restype = ctypes.c_int

    def probe(group):
        count = 32
        while True:
            pids = (ctypes.c_int * count)()
            used = lib.proc_listpids(2, group, pids, ctypes.sizeof(pids))
            if used <= 0:
                raise OSError(ctypes.get_errno(), "proc_listpids")
            if used < ctypes.sizeof(pids):
                break
            count *= 2
        total = 0
        for pid in list(pids)[:used // ctypes.sizeof(ctypes.c_int)]:
            if pid <= 0:
                continue
            usage = (ctypes.c_uint64 * 20)()
            if lib.proc_pid_rusage(pid, 2, usage):
                code = ctypes.get_errno()
                if code == errno.ESRCH:
                    continue
                raise OSError(code, "proc_pid_rusage")
            total += usage[9]  # ri_phys_footprint, not ri_resident_size.
        return total

    return probe


def kernel_peak(text, platform):
    if platform != "darwin":
        raise ValueError("kernel peak requires macOS")
    match = re.search(r"(\d+)\s+peak memory footprint", text)
    if not match:
        raise ValueError("missing kernel peak memory footprint")
    return int(match[1])


def check(peaks, peak, code):
    if code:
        raise ValueError(f"solver exit {code}")
    missing = [phase for phase in PHASES if not peaks.get(phase)]
    if missing:
        raise ValueError(f"missing phases: {','.join(missing)}")
    if max(peak, *peaks.values()) > LIMIT:
        raise ValueError(f"memory limit exceeded: {max(peak, *peaks.values())} > {LIMIT}")


def measure(solver, output, n, threads, timeout):
    probe = native_probe()
    output.mkdir(parents=True, exist_ok=False)
    inp = output / "box.inp"
    box(inp, n)
    run = output / "run"
    env = dict(os.environ, FH_THREADS=str(threads))
    command = ["/usr/bin/time", "-l", "-o", str(output / "time.txt"),
               str(solver), str(inp), str(run)]
    peaks = dict.fromkeys(PHASES, 0)
    phase = "read"
    samples, probe_seconds = 0, 0.0
    start = time.monotonic()
    cpu_start = resource.getrusage(resource.RUSAGE_SELF)
    with (output / "stderr.txt").open("w") as errors, \
            (output / "stdout.txt").open("wb") as log, \
            (output / "samples.csv").open("w") as csv:
        csv.write("seconds,phase,footprint_bytes\n")
        process = subprocess.Popen(command, env=env, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=errors,
                                   start_new_session=True)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        pending = b""
        next_probe = start
        try:
            while process.poll() is None or selector.get_map():
                now = time.monotonic()
                if now - start > timeout:
                    raise ValueError(f"solver exceeded {timeout} s")
                for key, _ in selector.select(max(0, min(0.1, next_probe - now))):
                    data = os.read(key.fd, 65536)
                    if not data:
                        selector.unregister(key.fileobj)
                        continue
                    log.write(data)
                    pending += data
                    while b"\n" in pending:
                        line, pending = pending.split(b"\n", 1)
                        text = line.decode("utf-8")
                        if text.startswith("estimate bytes="):
                            phase = "assemble"
                        elif text.startswith("progress "):
                            name = re.search(r"phase=(\w+)", text).group(1)
                            if name in ("assemble", "recover"):
                                phase = name
                            elif name == "solve":
                                phase = "iterate"  # Includes the PCG preconditioner or factor.
                        elif text.startswith("step "):
                            phase = "write"
                now = time.monotonic()
                if now >= next_probe and process.poll() is None:
                    before = time.monotonic()
                    try:
                        memory = probe(process.pid)
                    except OSError:
                        if process.poll() is not None:
                            break
                        raise
                    probe_seconds += time.monotonic() - before
                    samples += 1
                    peaks[phase] = max(peaks[phase], memory)
                    csv.write(f"{now - start:.6f},{phase},{memory}\n")
                    if memory > LIMIT:
                        raise ValueError(f"memory limit exceeded: {memory} > {LIMIT}")
                    next_probe = now + 0.1
            code = process.wait()
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            process.stdin.close()
            process.stdout.close()
            selector.close()
    peak = kernel_peak((output / "time.txt").read_text(), sys.platform)
    check(peaks, peak, code)
    cpu = resource.getrusage(resource.RUSAGE_SELF)
    cpu_seconds = cpu.ru_utime + cpu.ru_stime - cpu_start.ru_utime - cpu_start.ru_stime
    report = (f"elements: {n ** 3}\nthreads: {threads}\nlimit_bytes: {LIMIT}\n"
              f"kernel_peak_bytes: {peak}\nsamples: {samples}\n"
              f"probe_wall_s: {probe_seconds:.6f}\nmonitor_cpu_s: {cpu_seconds:.6f}\n"
              f"wall_s: {time.monotonic() - start:.3f}\n"
              "phases[5]{phase,peak_bytes}:\n" +
              "".join(f"  {name},{peaks[name]}\n" for name in PHASES) + "status: pass\n")
    (output / "memory.toon").write_text(report)
    print(report, end="")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--solver", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new scratch folder")
    parser.add_argument("--n", type=int, default=100, help="elements on each axis")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--timeout", type=float, default=1800)
    args = parser.parse_args()
    if args.n < 1 or args.threads < 1 or args.timeout <= 0:
        parser.error("n, threads and timeout must be positive")
    try:
        measure(args.solver.resolve(strict=True), args.output.resolve(),
                args.n, args.threads, args.timeout)
    except (OSError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
