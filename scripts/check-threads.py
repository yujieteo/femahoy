#!/usr/bin/env python3
"""Compare the result bytes of fh-solve at several thread counts (E3, decision D-11).

Each input runs once for each thread count (FH_THREADS) with all 3 result formats. Each input
also runs with a kill after the first restart file, then a restart in the same folder. Every
file of each run folder must have the same bytes as the full run on the first thread count:
the displacements U and the residual forces RF (internal force minus load at each node) in
the result files, the stresses, the PVD, the restart files and the summary. The summary lines
wall_s and run_id change from run to run, so the comparison leaves them out. The step lines
of the full runs must also be equal. The solver writes no file of the matrix.

An input with one step gets a copy of that step with twice the loads, so that the restart
point is inside the run. --box N adds the C3D8 box of check-phase-memory.py with N elements
on each axis. Above 60 000 free dofs, the solver uses PCG instead of Cholesky.

The peak memory is the maximum resident set size of the solver process, with the emulator.
With GNU time (Linux), time is the parent of the solver. A child process starts with the
memory peak of its parent, and time is small, so the value does not include this script.
"""

import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import shlex
import signal
import subprocess
import sys
import threading
import time

FORMATS = "text,vtu,vtk"


def gnu_time():
    """The GNU time command that writes the peak memory in KiB to a file, or None."""
    try:
        probe = subprocess.run(["/usr/bin/time", "-f", "%M", "-o", os.devnull, "true"],
                               capture_output=True, check=False)
    except OSError:
        return None
    return ["/usr/bin/time", "-f", "%M", "-o"] if probe.returncode == 0 else None


TIMER = gnu_time()


def box_input(path, n):
    """Write the box of the memory gate (scripts/check-phase-memory.py)."""
    spec = importlib.util.spec_from_file_location(
        "phase_memory", Path(__file__).with_name("check-phase-memory.py"))
    gate = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gate)
    gate.box(path, n)


def with_two_steps(text):
    """Add a copy of the only step, with twice the loads, to an input with one step."""
    lines = text.splitlines()
    keys = [line.replace(" ", "").upper() for line in lines]
    starts = [i for i, key in enumerate(keys) if key.startswith("*STEP")]
    if len(starts) != 1:
        return text
    end = next(i for i in range(starts[0], len(keys)) if keys[i].startswith("*ENDSTEP"))
    copy, keyword = [], ""
    for line in lines[starts[0]:end + 1]:
        if line.startswith("*") and not line.startswith("**"):
            keyword = line.split(",")[0].strip().upper()
            if keyword == "*STEP":
                line = re.sub(r"(?i)(NAME\s*=\s*)([^,\s]+)", r"\g<1>\g<2>2", line)
        elif not line.startswith("**"):
            fields = line.split(",")
            # The value is the last field of a load, and the fourth field of a boundary.
            at = {"*CLOAD": len(fields) - 1, "*DSLOAD": len(fields) - 1, "*BOUNDARY": 3}.get(keyword)
            if at is not None and at < len(fields) and fields[at].strip():
                fields[at] = repr(2 * float(fields[at]))
                line = ",".join(fields)
        copy.append(line)
    return "\n".join(lines + copy) + "\n"


class Run:
    """One fh-solve process: exit code, kill flag, elapsed seconds, peak RSS bytes, stdout."""

    def __init__(self, code, killed, timed_out, elapsed, peak, lines):
        self.code, self.killed, self.timed_out = code, killed, timed_out
        self.elapsed, self.peak, self.lines = elapsed, peak, lines


def solve(command, inp, run_dir, threads, log, timeout, restart=False, kill=False):
    """Run fh-solve with FH_THREADS=threads. `kill` stops it at the first restart line."""
    timer = [*TIMER, f"{log}.time"] if TIMER else []
    args = [*timer, *command, str(inp), str(run_dir), "--formats", FORMATS]
    if restart:
        args.append("--restart")
    env = dict(os.environ, FH_THREADS=str(threads))
    lines, killed, timed_out = [], False, []
    start = time.monotonic()
    with open(f"{log}.stderr", "w") as err:
        # An open stdin keeps the solver running: the end of stdin is a cancel.
        process = subprocess.Popen(args, env=env, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=err, text=True)

        def stop():
            # Kill the solver. Under GNU time, kill the child of time, so time writes the peak.
            pid = process.pid
            try:
                pid = int(Path(f"/proc/{pid}/task/{pid}/children").read_text().split()[0])
            except (OSError, IndexError, ValueError):
                pass
            os.kill(pid, signal.SIGKILL)

        watchdog = threading.Timer(timeout, lambda: (timed_out.append(1), stop()))
        watchdog.start()
        for line in process.stdout:
            lines.append(line)
            if kill and not killed and line.startswith("restart "):
                stop()
                killed = True
        _, status, usage = os.wait4(process.pid, 0)
        watchdog.cancel()
        process.returncode = os.waitstatus_to_exitcode(status)
        process.stdin.close()
    elapsed = time.monotonic() - start
    Path(f"{log}.stdout").write_text("".join(lines))
    if timer:
        # The last word of the file is the peak in KiB, after a line about a signal or an exit code.
        try:
            peak = int(Path(f"{log}.time").read_text().split()[-1]) * 1024
        except (OSError, IndexError, ValueError):
            peak = 0
    else:
        # ru_maxrss counts KiB on Linux and bytes on macOS.
        peak = usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024)
    return Run(process.returncode, killed, bool(timed_out), elapsed, peak, lines)


def files(run_dir):
    """The SHA-256 of each file of a run folder, without the summary lines that change. The
    script keeps no file in memory, so that its own memory stays small."""
    out = {}
    for path in sorted(run_dir.iterdir()):
        if not path.is_file():
            continue
        h = hashlib.sha256()
        if path.name == "summary.toon":
            h.update(b"\n".join(line for line in path.read_bytes().split(b"\n")
                                if b"wall_s:" not in line and not line.startswith(b"run_id:")))
        else:
            with path.open("rb") as f:
                for block in iter(lambda: f.read(1 << 20), b""):
                    h.update(block)
        out[path.name] = h.hexdigest()
    return out


def digest(snapshot):
    h = hashlib.sha256()
    for name, sha in snapshot.items():
        h.update(f"{name}\0{sha}\n".encode())
    return h.hexdigest()


def step_lines(run):
    return [line.strip() for line in run.lines if line.startswith("step ")]


def solver_facts(steps):
    """Method, free dofs and PCG iterations of each step, from the step lines."""
    def field(name, join=False):
        values = [dict(w.split("=", 1) for w in line.split() if "=" in w)[name] for line in steps]
        return "+".join(values) if join or len(set(values)) > 1 else values[0]
    if not steps:
        return ("?",) * 3
    return field("method"), field("free_dofs"), field("iterations", join=True)


def check_case(name, inp, out, command, counts, timeout, report):
    """Run one input at each thread count. Append the rows to `report`. True when all agree."""
    ok, reference, ref_steps = True, None, None
    for kind in ("full", "restart"):
        for t in counts:
            run_name = f"{kind}-{t}"
            run_dir, log = out / run_name, out / run_name
            error = None
            if kind == "full":
                runs = [solve(command, inp, run_dir, t, log, timeout)]
            else:
                runs = [solve(command, inp, run_dir, t, f"{log}-killed", timeout, kill=True)]
                if not runs[0].killed:
                    error = "the run ended before the first restart file"
                else:
                    runs.append(solve(command, inp, run_dir, t, log, timeout, restart=True))
            last = runs[-1]
            if error is None and (last.timed_out or last.code != 0):
                error = "timeout" if last.timed_out else f"exit {last.code}, see {log}.stderr"
            elapsed = sum(r.elapsed for r in runs)
            peak = max(r.peak for r in runs)
            if error:
                print(f"FAIL {name} {run_name}: {error}", file=sys.stderr)
                ok = False
                report.append((name, kind, t, "?", "?", "?", elapsed, peak, 0, "-", "no"))
                continue
            snapshot = files(run_dir)
            if reference is None:
                reference, ref_steps = snapshot, step_lines(last)
            differ = sorted(set(snapshot) ^ set(reference))
            differ += [f for f in snapshot if f in reference and snapshot[f] != reference[f]]
            if kind == "full" and step_lines(last) != ref_steps:
                differ.append("step lines")
            if differ:
                print(f"FAIL {name} {run_name}: not the bytes of the first full run: "
                      + ", ".join(differ), file=sys.stderr)
                ok = False
            method, dofs, iterations = solver_facts(ref_steps)
            report.append((name, kind, t, method, dofs, iterations, elapsed, peak,
                           len(snapshot), digest(snapshot)[:16], "no" if differ else "yes"))
            with open(out.parent / "digests.txt", "a") as d:
                for f, sha in snapshot.items():
                    d.write(f"{name}\t{run_name}\t{sha}\t{f}\n")
    return ok


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--solver", type=Path, required=True, help="fh-solve")
    parser.add_argument("--output", type=Path, required=True, help="new folder")
    parser.add_argument("--emulator", default="", help='for example "qemu-x86_64 -cpu Nehalem"')
    parser.add_argument("--threads", default="1,2,4,6", help="thread counts")
    parser.add_argument("--box", type=int, action="append", default=[], metavar="N")
    parser.add_argument("--timeout", type=float, default=3600, help="seconds for one run")
    parser.add_argument("inputs", type=Path, nargs="*", help="solver input files")
    args = parser.parse_args()
    counts = [int(t) for t in args.threads.split(",")]
    if not counts or min(counts) < 1 or not (args.inputs or args.box):
        parser.error("give thread counts of 1 or more, and an input or a box")
    command = [*shlex.split(args.emulator), str(args.solver.resolve(strict=True))]
    args.output.mkdir(parents=True)
    cases = []
    for path in args.inputs:
        name = path.resolve().parent.name
        while name in [c[0] for c in cases]:
            name += "+"
        cases.append((name, with_two_steps(path.read_text())))
    for n in args.box:
        tmp = args.output / f"box-{n}.tmp"
        box_input(tmp, n)
        cases.append((f"box-{n}", with_two_steps(tmp.read_text())))
        tmp.unlink()
    report, ok = [], True
    for name, text in cases:
        out = args.output / name
        out.mkdir()
        inp = out / f"{name}.inp"
        inp.write_text(text)
        ok &= check_case(name, inp, out, command, counts, args.timeout, report)
    clock = " (emulator)" if args.emulator else ""
    table = [f"Solver: {shlex.join(command)}", "",
             f"| Input | Run | Threads | Method | Free dofs | PCG iterations | Elapsed s{clock} "
             f"| Peak RSS MiB{clock} | Files | Digest | Same bytes as full-{counts[0]} |",
             "|---|---|---:|---|---|---|---:|---:|---:|---|---|"]
    tsv = ["input\trun\tthreads\tmethod\tfree_dofs\titerations\telapsed_s\tpeak_rss_mib\tfiles\tdigest\tsame"]
    for r in report:
        cells = [*map(str, r[:6]), f"{r[6]:.2f}", f"{r[7] / 2**20:.1f}", *map(str, r[8:])]
        table.append("| " + " | ".join(cells) + " |")
        tsv.append("\t".join(cells))
    (args.output / "threads.md").write_text("\n".join(table) + "\n")
    (args.output / "threads.tsv").write_text("\n".join(tsv) + "\n")
    print("\n".join(table))
    print("PASS: the same bytes at every thread count." if ok else "FAIL: see the lines above.")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
