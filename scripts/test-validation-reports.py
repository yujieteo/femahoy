"""Protect explicit units, zero references, separate mesh series and plot files in reports."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('reports', ROOT / 'scripts/validation-reports.py')
reports = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reports)


class ReportResults(unittest.TestCase):
    def test_hand_check_has_explicit_units_and_current_values(self):
        page = reports.text('contact-hertz', 'pass', [['p0', 6379.47, 6363.06, 190.8918, 174.4818]], 'test')
        self.assertIn('MPa', page.split('## Result')[1].split('## Source')[0])
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
        page = reports.text('naca23012-flap', 'not run: OpenFOAM is not installed', [], 'test')
        with tempfile.TemporaryDirectory() as tmp:
            files = reports.mesh_curves('naca23012-flap', Path(tmp))
            self.assertEqual(sorted(re.findall(r'<!--curve (mesh-\S+)-->', page)), [f.name for f in files])
            self.assertEqual(len(reports.curve_files(files)), 6)

    def test_zero_reference_has_absolute_difference(self):
        self.assertEqual(reports.difference(2, 0, 'mm'), '+2 mm (absolute)')
        self.assertEqual(reports.difference(-2, -4, 'MPa'), '+2 MPa (+50 %)')
        self.assertEqual(reports.difference(2, 1, ''), '+1 (+100 %)')

    def test_plot_file_is_standalone_with_legend_and_notes(self):
        figure = ('<figure class="curve"><figcaption>Front &amp; time</figcaption><svg viewBox="0 0 640 360" role="img">'
                  '<polyline style="stroke:var(--s2)" points="1,2"/></svg><div data-control="values" hidden><div hidden>'
                  '<p class="keys">keys</p><table><caption>Original values. — means no sample.</caption><thead><tr>'
                  '<th scope="col">T [1]</th><th scope="col">model (Z [1])</th></tr></thead><tbody><tr data-x="72">'
                  '<td>0.28</td><td>1.0000001</td></tr><tr data-x="80"><td>1.19</td><td>—</td></tr></tbody></table></div></div><ul class="legend"><li><i class="line" style="background:var(--s1)"></i>model</li>'
                  '<li><i class="point" style="background:var(--s2)"></i>measured</li></ul><p>measured: Martin</p><p>A note.</p></figure>')
        title, file, lines, samples = reports.svg(figure)
        self.assertEqual(title, 'Front & time')
        self.assertEqual(lines, ['measured: Martin', 'A note.'])
        self.assertNotIn('var(--', file)
        self.assertIn('viewBox="0 0 640 404"', file)
        self.assertIn('>measured</text>', file)
        self.assertIn('| T [1] | model (Z [1]) |', samples)
        self.assertIn('| 0.28 | 1.0000001 |', samples)
        self.assertIn('| 1.19 | — |', samples)


if __name__ == '__main__':
    unittest.main()
