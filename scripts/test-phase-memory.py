#!/usr/bin/env python3
"""Checks for the phase memory gate. No large solve is required."""

import importlib.util
import contextlib
import io
from pathlib import Path
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "phase_memory", Path(__file__).with_name("check-phase-memory.py")
)
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class MemoryGateTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "darwin", "macOS footprint API")
    def test_monitor_records_all_phases_and_kernel_peak(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            solver = root / "solver.py"
            solver.write_text(
                "#!/usr/bin/env python3\nimport time\n"
                "time.sleep(0.2)\n"
                "for line in ('estimate bytes=1', "
                "'progress step=1/1 phase=solve fraction=0', "
                "'progress step=1/1 phase=recover fraction=0', 'step 1'):\n"
                "    print(line, flush=True)\n    time.sleep(0.2)\n"
            )
            solver.chmod(0o755)
            with contextlib.redirect_stdout(io.StringIO()):
                gate.measure(solver, root / "proof", 1, 1, 10)
            report = (root / "proof" / "memory.toon").read_text()
            self.assertIn("status: pass", report)
            self.assertIn("kernel_peak_bytes:", report)

    def test_missing_phase_fails(self):
        with self.assertRaisesRegex(ValueError, "missing phases"):
            gate.check({"read": 1}, 1, 0)

    def test_kernel_peak_above_limit_fails(self):
        peaks = dict.fromkeys(gate.PHASES, 1)
        with self.assertRaisesRegex(ValueError, "memory limit"):
            gate.check(peaks, gate.LIMIT + 1, 0)
        gate.check(peaks, gate.LIMIT, 0)

    def test_sample_above_limit_fails(self):
        peaks = dict.fromkeys(gate.PHASES, 1)
        peaks["iterate"] = gate.LIMIT + 1
        with self.assertRaisesRegex(ValueError, "memory limit"):
            gate.check(peaks, 1, 0)

    def test_failed_solver_fails(self):
        with self.assertRaisesRegex(ValueError, "solver exit"):
            gate.check(dict.fromkeys(gate.PHASES, 1), 1, 130)

    def test_time_peak_is_footprint_on_macos(self):
        text = "  123 maximum resident set size\n  456 peak memory footprint\n"
        self.assertEqual(gate.kernel_peak(text, "darwin"), 456)
        with self.assertRaisesRegex(ValueError, "kernel peak"):
            gate.kernel_peak("", "darwin")

    def test_box_has_correct_connectivity_and_size(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "box.inp"
            gate.box(path, 2)
            text = path.read_text()
            nodes, elements = text.split("*ELEMENT, TYPE=C3D8\n")
            self.assertEqual(len(nodes.splitlines()) - 1, 27)
            element_lines = elements.split("*ELSET")[0].splitlines()
            self.assertEqual(len(element_lines), 8)
            self.assertEqual(element_lines[0], "1,1,2,5,4,10,11,14,13")
            self.assertIn("*END STEP", text)


if __name__ == "__main__":
    unittest.main()
