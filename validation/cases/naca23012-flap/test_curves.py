"""Small result-safety checks. No solver process runs."""
import importlib.util
from pathlib import Path
import unittest
import tempfile

spec = importlib.util.spec_from_file_location("flap_curves", Path(__file__).with_name("curves.py"))
curves = importlib.util.module_from_spec(spec)
spec.loader.exec_module(curves)


class ResultSafety(unittest.TestCase):
    def test_initial_residuals_preserve_gaps_and_replace_duplicate_blocks(self):
        self.assertEqual(curves.residual_history(""), [])
        rows = curves.residual_history("""Time = 1
smoothSolver: Solving for Ux, Initial residual = 0.2, Final residual = 0.0001, No Iterations 2
GAMG: Solving for p, Initial residual = 0.1, Final residual = 0.001, No Iterations 2
Time = 1
GAMG: Solving for p, Initial residual = 0.04, Final residual = 0.001, No Iterations 2
Time = 2
GAMG: Solving for p, Initial residual = broken, Final residual = 0.001, No Iterations 2
smoothSolver: Solving for nuTilda, Initial residual = 1e-7, Final residual = 0, No Iterations 2
""")
        self.assertEqual(rows, [[1.0, {"p": 0.04}], [2.0, {"nuTilda": 1e-7}]])

    def test_force_history_keeps_full_precision_and_replaces_duplicate_iterations(self):
        with tempfile.TemporaryDirectory(prefix="fh-flap-history-", dir="/tmp") as folder:
            foam = Path(folder)
            self.assertEqual(curves.force_history(foam), [])
            data = foam / "postProcessing/forceCoeffs/0/coefficient.dat"
            data.parent.mkdir(parents=True)
            data.write_text("# Time Cl CmPitch\n1 0.123456789 -0.01\n2 0.2 -0.02\n2 0.21 -0.021\nbroken\n")
            self.assertEqual(curves.force_history(foam), [[1.0, .123456789, -.01], [2.0, .21, -.021]])

    def test_wall_geometry_keeps_two_values_near_the_hinge(self):
        loop = [(1,0,0), (.79,.03,1.2), (.3,.08,.4), (0,0,0), (.3,-.04,-.2), (.79,-.02,-.8)]
        surfaces, wall = curves.split_wall(loop)
        self.assertIn([.79,1.2], surfaces["upper"])
        self.assertIn([.79,-.8], surfaces["lower"])
        reverse, _ = curves.split_wall(list(reversed(loop)))
        self.assertIn([.79,1.2], reverse["upper"])
        self.assertIn([.79,-.8], reverse["lower"])

    def test_uniform_velocity_crosses_only_shared_fluid_edges(self):
        cells = [[(0,0),(1,0),(1,1),(0,1)], [(1,0),(2,0),(2,1),(1,1)]]
        trace = curves.traces(cells, [(1,0,0)]*2, [(0.25,0.5)])[0]
        self.assertEqual(trace["points"], [[0.25,0.5],[1.0,0.5],[2.0,0.5]])
        self.assertEqual(trace["stop"], "wall or outer boundary")

    def test_thin_wall_stops_trace_before_other_fluid_cell(self):
        cells = [[(0,0),(1,0),(1,1),(0,1)], [(1.000001,0),(2,0),(2,1),(1.000001,1)]]
        trace = curves.traces(cells, [(1,0,0)]*2, [(0.25,0.5)])[0]
        self.assertEqual(trace["points"][-1], [1.0,0.5])
        self.assertEqual(trace["stop"], "wall or outer boundary")
        self.assertEqual(curves.traces(cells, [(0,0,0)]*2, [(0.25,0.5)])[0]["stop"], "near zero speed")
        self.assertEqual(curves.traces(cells, [(1,0,0)]*2, [(3,0.5)])[0]["stop"], "outside mesh")


if __name__ == "__main__":
    unittest.main()
