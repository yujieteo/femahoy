"""Protect explicit units, zero references, and separate mesh series in reports."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('reports', ROOT / 'scripts/validation-reports.py')
reports = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reports)


class ReportResults(unittest.TestCase):
    def test_hand_check_has_explicit_units_and_current_values(self):
        page = reports.text('contact-hertz', 'pass', [['p0', 6379.47, 6363.06, 190.8918, 174.4818]], [], ['job'], 'test')
        self.assertIn('MPa', page.split('<h2>Result</h2>')[1].split('<h2>Source</h2>')[0])
        self.assertIn('+16.41 MPa', page)
        self.assertIn('+0.257895 %', page)
        self.assertEqual(page.count('E* = 210000'), 1)

    def test_mesh_plot_and_table_share_only_3000_iteration_rows(self):
        page, curves = reports.mesh_report('naca23012-flap')
        self.assertEqual(len(curves), 6)
        for csv in curves.values():
            rows = csv.splitlines()[-3:]
            self.assertEqual([r.split(',')[0] for r in rows], ['9216', '36864', '147456'])
        self.assertIn('0.12846', page)
        self.assertNotIn('0.12888', page)
        self.assertIn('No extrapolation', page)

    def test_not_run_mesh_trend_has_a_file_for_each_curve_mark(self):
        import re, tempfile
        page = reports.text('naca23012-flap', 'not run: OpenFOAM is not installed', [], None, [], 'test')
        with tempfile.TemporaryDirectory() as tmp:
            files = reports.mesh_curves('naca23012-flap', Path(tmp))
            self.assertEqual(sorted(re.findall(r'<!--curve (mesh-\S+)-->', page)), [f.name for f in files])
            self.assertEqual(len(reports.curve_files(files)), 6)

    def test_zero_reference_has_absolute_difference(self):
        self.assertEqual(reports.difference(2, 0, 'mm'), '+2 mm (absolute)')
        self.assertEqual(reports.difference(-2, -4, 'MPa'), '+2 MPa (+50 %)')
        self.assertEqual(reports.difference(2, 1, ''), '+1 (+100 %)')


if __name__ == '__main__':
    unittest.main()
