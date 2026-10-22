#!/usr/bin/env python3
"""Write 1 report of `fh report` for each validation case to validation/reports/.

    scripts/validation-reports.py [CASE]...

CASE is the folder name of a case in validation/cases. With no CASE, the script does all the
cases. For each case:

1. `fh validate` solves the case and gives the computed value of each check.
2. `fh run` solves each input of the case again, for the 3D view of the report. A fit case gets
   the plot of its fit (`fh fit -o`) in place of a run. Each curve file (a CSV file with a
   `# y:` line) in the case folder or in the output of a run is a plot of the report.
3. `fh report --viewer viewer.js` writes validation/reports/CASE.html: a table of the checks,
   then derivation.md, then the runs.

A case that cannot run (its solver does not start on this computer) gets no report. The index
names the case and the reason. A case that fails gets its report, which shows the failure. An
input that writes no field (a CalculiX input with no *NODE FILE and no *EL FILE) has no 3D view.
A report above 5 MB is made again with only the last step of each run (`fh report --last-step`).
A report above 10 MB is not kept: the index names its size.

The reports share 1 copy of the wasm viewer, validation/reports/viewer.js. A report opens with a
double-click when viewer.js is beside it. outcomes.tsv keeps the result of each case, so a run
for some cases keeps the lines of the other cases. index.html links all the reports.

Needs `fh` (target/release/fh, or $FH_BIN) and the wasm viewer (target/release/viewer, or
$FH_VIEWER, from scripts/build-viewer.sh). The runs go to a temporary state folder.
Only the Python standard library.
"""

import base64
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zlib
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "validation/cases"
OUT = ROOT / "validation/reports"
FONTS = ROOT / "crates/fh/src/daemon/fonts"
FH = Path(os.environ.get("FH_BIN") or ROOT / "target/release/fh")
VIEWER = Path(os.environ.get("FH_VIEWER") or ROOT / "target/release/viewer")
VIEWER_FILES = ["fh_viewer.js", "fh_viewer_bg.wasm"]
# A report above SMALL bytes keeps only the last step of each run. The repository keeps no report
# above LIMIT bytes.
SMALL = 5_000_000
LIMIT = 10_000_000


def esc(text):
    return html.escape(text, quote=True)


# Markdown: the subset that derivation.md uses. Titles, paragraphs, tables, lists (with
# continuation lines and nested lists), fenced code, `code`, **strong**, *emphasis*, <url> and
# bare https:// links.

CODE_SPAN = re.compile(r"(`+)(.+?)(?<!`)\1(?!`)")
URL = re.compile(r"&lt;(https?://[^\s&]+)&gt;|(https?://[^\s<]+[^\s<.,;:)])")


def inline(text):
    parts = []
    last = 0
    for m in CODE_SPAN.finditer(text):
        parts.append(words(text[last : m.start()]))
        parts.append(f"<code>{esc(m.group(2).strip())}</code>")
        last = m.end()
    parts.append(words(text[last:]))
    return "".join(parts)


def words(text):
    text = esc(text)
    text = URL.sub(lambda m: f'<a href="{m[1] or m[2]}">{m[1] or m[2]}</a>', text)
    text = re.sub(r"\*\*(\S(?:.*?\S)?)\*\*", r"<strong>\1</strong>", text)
    return re.sub(r"(?<![\w*])\*(\S(?:.*?\S)?)\*(?![\w*])", r"<em>\1</em>", text)


def cells(line):
    """The cells of a table row. A `|` in a code span is not a cell boundary."""
    out, cell, code = [], "", False
    for c in line.strip().strip("|"):
        if c == "`":
            code = not code
        if c == "|" and not code:
            out.append(cell.strip())
            cell = ""
        else:
            cell += c
    return out + [cell.strip()]


ITEM = re.compile(r"( *)([-*]|\d+\.) (.*)")


def markdown(text):
    lines = text.splitlines()
    out, i = [], 0
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
        elif line.startswith("```"):
            j = i + 1
            while j < len(lines) and not lines[j].startswith("```"):
                j += 1
            out.append("<pre><code>" + esc("\n".join(lines[i + 1 : j])) + "</code></pre>")
            i = j + 1
        elif m := re.match(r"(#{1,6}) (.*)", line):
            n = len(m[1])
            out.append(f"<h{n}>{inline(m[2])}</h{n}>")
            i += 1
        elif line.startswith("|") and i + 1 < len(lines) and re.match(r"\|[-:| ]+\|$", lines[i + 1].strip()):
            align = ["right" if c.endswith(":") else "left" for c in cells(lines[i + 1])]
            head = "".join(f'<th style="text-align:{a}">{inline(c)}</th>' for c, a in zip(cells(line), align))
            rows = []
            i += 2
            while i < len(lines) and lines[i].startswith("|"):
                rows.append("".join(f'<td style="text-align:{a}">{inline(c)}</td>' for c, a in zip(cells(lines[i]), align)))
                i += 1
            body = "".join(f"<tr>{r}</tr>" for r in rows)
            out.append(f"<table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>")
        elif ITEM.match(line):
            i = lists(lines, i, out)
        else:
            j = i
            while j < len(lines) and lines[j].strip() and not re.match(r"#{1,6} |```|\||( *)([-*]|\d+\.) ", lines[j]):
                j += 1
            out.append("<p>" + inline("\n".join(lines[i:j])) + "</p>")
            i = j
    return "\n".join(out)


def lists(lines, i, out):
    """Add the list that starts at line i to out. Give the line after it."""
    items = []  # (indent, ordered, number, text)
    while i < len(lines) and lines[i].strip():
        m = ITEM.match(lines[i])
        if m:
            items.append([len(m[1]), m[2][0].isdigit(), m[2].rstrip("."), m[3]])
        elif lines[i].startswith(" "):
            items[-1][3] += "\n" + lines[i].strip()
        else:
            break
        i += 1
    out.append(nest(items, 0, len(items)))
    return i


def nest(items, a, b):
    indent, ordered = items[a][0], items[a][1]
    tag = "ol" if ordered else "ul"
    start = f' start="{items[a][2]}"' if ordered and items[a][2] != "1" else ""
    html_items, k = [], a
    while k < b:
        j = k + 1
        while j < b and items[j][0] > indent:
            j += 1
        inner = nest(items, k + 1, j) if j > k + 1 else ""
        html_items.append(f"<li>{inline(items[k][3])}{inner}</li>")
        k = j
    return f"<{tag}{start}>" + "".join(html_items) + f"</{tag}>"


# The cases.


def fh(*args, env):
    return subprocess.run([str(FH), *map(str, args)], env=env, capture_output=True, text=True)


def validate(case, tmp, env):
    """The status line and the rows of the checks of `fh validate` for 1 case."""
    root = tmp / f"root-{case}"
    (root / "validation/cases").mkdir(parents=True)
    (root / "validation/cases" / case).symlink_to(CASES / case)
    r = fh("validate", "--root", root, env=env)
    status, rows = None, []
    for line in r.stdout.splitlines():
        if line.startswith(case + "\t"):
            status = line.split("\t", 1)[1]
        elif line.startswith("  " + case + "\t"):
            f = line.strip().split("\t")
            rows.append([f[1]] + [float(x.split("=")[1]) for x in f[2:]])
    if status is None:
        status = "fail: " + (r.stderr.strip() or r.stdout.strip() or f"fh validate exit {r.returncode}")
    return status, rows


def inputs(case):
    """The inputs of a case for `fh run`: job.inp, or each subfolder that a check names."""
    checks = tomllib.loads((CASES / case / "check.toml").read_text())
    folders = []
    for v in checks.values():
        if isinstance(v, dict) and "case" in v and v["case"] not in folders:
            folders.append(v["case"])
    return [CASES / case / f for f in folders] or [CASES / case / "job.inp"]


def curve_files(files):
    return [f for f in sorted(files) if "\n# y:" in "\n" + f.read_text(errors="replace")[:4096]]


def solve(case, tmp, env):
    """The runs, the curves, and the inputs with no field."""
    runs, curves, nofield = [], curve_files((CASES / case).glob("*.csv")), []
    for target in inputs(case):
        name = target.relative_to(CASES / case).as_posix()
        fit = target / "fit.toml"
        if fit.is_file():
            f = tomllib.loads(fit.read_text())
            curve = tmp / f"{case}-{target.name}-fit.csv"
            series = ["--series", f["series"]] if "series" in f else []
            r = fh("fit", f["model"], target / f["curve"], *series, "-o", curve, env=env)
            if r.returncode != 0:
                raise RuntimeError(f"fh fit {name}: {r.stderr.strip()}")
            curves.append(curve)
            continue
        print(f"  fh run {name}", flush=True)
        r = fh("run", target, env=env)
        m = re.search(r"fh run: (run-\d+) succeeded", r.stdout + r.stderr)
        if not m:
            tail = "\n".join((r.stdout + r.stderr).strip().splitlines()[-5:])
            raise RuntimeError(f"fh run {name}: {tail}")
        if "has no field to show" in r.stdout + r.stderr:
            nofield.append(name)
            continue
        runs.append((m[1], name))
        curves += curve_files((Path(env["FH_STATE"]) / "runs" / m[1] / "out").rglob("*.csv"))
    return runs, curves, nofield


def text(case, status, rows, runs, nofield, version, last_step=False):
    checks = tomllib.loads((CASES / case / "check.toml").read_text())
    result = status.split(":", 1)[0]
    body = [
        "<h2>Result</h2>",
        f"<p><strong>{esc(result.capitalize())}.</strong> "
        f"A check passes when |computed - reference| &le; rel_tol &middot; |reference| + abs_tol, "
        f"with rel_tol = {checks.get('rel_tol', 0)} and abs_tol = {checks.get('abs_tol', 0)} "
        f"(<code>check.toml</code>). <code>fh validate</code> gives the computed values.</p>",
    ]
    if status != "pass":
        body.append(f"<pre><code>{esc(status)}</code></pre>")
    if rows:
        head = "".join(f'<th style="text-align:{a}">{h}</th>' for h, a in [
            ("Check", "left"), ("Computed", "right"), ("Reference", "right"),
            ("Tolerance", "right"), ("Margin", "right")])
        trs = "".join(
            f"<tr><td><code>{esc(r[0])}</code></td>"
            + "".join(f'<td style="text-align:right">{v:.6g}</td>' for v in r[1:])
            + "</tr>"
            for r in rows
        )
        body.append(f"<table><thead><tr>{head}</tr></thead><tbody>{trs}</tbody></table>")
    if runs:
        links = ", ".join(f'<a data-run="{run}">{esc(name)}</a>' for run, name in runs)
        last = " Only the last step of each run is in this report, to keep the file small." if last_step else ""
        body.append(f"<p>The 3D view shows the run of each input: {links}.{last}</p>")
    if nofield:
        names = ", ".join(f"<code>{esc(n)}</code>" for n in nofield)
        body.append(f"<p>{names} writes no field (no <code>*NODE FILE</code> and no <code>*EL FILE</code>), "
                    "so this report has no 3D view of it.</p>")
    body.append(f"<p>Made by <code>scripts/validation-reports.py</code> on {date.today()}, femahoy {esc(version)}.</p>")
    derivation = markdown((CASES / case / "derivation.md").read_text())
    # The result goes after the title of derivation.md.
    title, _, rest = derivation.partition("\n")
    return title + "\n" + "\n".join(body) + "\n" + rest


def deflate(data):
    """Raw deflate, the format of the files in a report (`deflate-raw` of DecompressionStream)."""
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


def write_viewer():
    files = {f"/viewer/{n}": base64.b64encode(deflate((VIEWER / n).read_bytes())).decode() for n in VIEWER_FILES}
    (OUT / "viewer.js").write_text(
        "// The wasm viewer of femahoy for the reports in this folder (fh report --viewer).\n"
        "// Each file as raw deflate in base64, under its daemon path. scripts/validation-reports.py\n"
        "// writes this file.\n"
        f"window.fhViewer = {json.dumps(files)};\n"
    )


def read_outcomes():
    path = OUT / "outcomes.tsv"
    if not path.is_file():
        return {}
    lines = path.read_text().splitlines()[1:]
    return {f[0]: f[1:] for f in (line.split("\t") for line in lines)}


def write_outcomes(outcomes):
    lines = ["case\tresult\tdetail\tbytes"]
    lines += ["\t".join([c, *outcomes[c]]) for c in sorted(outcomes)]
    (OUT / "outcomes.tsv").write_text("\n".join(lines) + "\n")


INDEX_STYLE = """
:root { --bg: #f4f1ea; --text: #1c1b18; --muted: #5c5950; --line: #d8d2c4; --accent: #c4613f;
  --serif: "Source Serif 4", Georgia, serif; --sans: "DM Sans", system-ui, sans-serif; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #1c1b18; --text: #ece8df; --muted: #a39e92; --line: #3a3730; --accent: #e0805c; }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 17px/1.55 var(--sans); }
main { max-width: 72rem; margin: 0 auto; padding: 3rem clamp(1rem, 4vw, 3rem) 5rem; }
h1 { font: 400 clamp(2.5rem, 6vw, 4rem)/1.06 var(--serif); letter-spacing: -0.015em; margin: 0; text-wrap: balance; }
.lede { font-size: 1.25rem; color: var(--muted); margin: 1.5rem 0 3rem; max-width: 44rem; text-wrap: pretty; }
a { color: inherit; text-decoration-thickness: 1px; text-underline-offset: 0.2em; }
a:hover { text-decoration-color: var(--accent); }
a:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
th, td { padding: 0.75rem 1rem 0.75rem 0; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; }
th { font-size: 0.8125rem; font-weight: 500; letter-spacing: 0.04em; text-transform: uppercase; color: var(--muted); border-bottom-color: var(--text); }
.n { text-align: right; white-space: nowrap; }
td small { display: block; color: var(--muted); }
code { font: 0.95em ui-monospace, monospace; overflow-wrap: anywhere; }
"""


def write_index(outcomes, version):
    faces = "".join(
        f'@font-face {{ font-family: "{family}"; font-weight: {weight}; src: url(data:font/woff2;base64,'
        f'{base64.b64encode((FONTS / file).read_bytes()).decode()}) format("woff2"); }}\n'
        for family, weight, file in [("Source Serif 4", "400", "source-serif-4.woff2"),
                                     ("DM Sans", "100 1000", "dm-sans.woff2")]
    )
    rows = []
    for case in sorted(p.name for p in CASES.iterdir() if (p / "check.toml").is_file()):
        result, detail, size = outcomes.get(case, ["not run", "", ""])
        name = f'<a href="{case}.html">{case}</a>' if (OUT / f"{case}.html").is_file() else case
        size = f"{int(size) / 1e6:.2f} MB" if size else ""
        note = f"<small>{esc(detail)}</small>" if detail else ""
        rows.append(f"<tr><td>{name}</td><td>{esc(result)}{note}</td><td class=n>{size}</td></tr>")
    passed = sum(1 for o in outcomes.values() if o[0] == "pass")
    (OUT / "index.html").write_text(f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<title>femahoy validation reports</title>
<style>
{faces}{INDEX_STYLE}</style>
</head>
<body>
<main>
<h1>Validation reports</h1>
<p class="lede">1 report for each validation case of femahoy: the checks, the derivation and a 3D view of the runs. {passed} of {len(rows)} cases pass. Made by <code>scripts/validation-reports.py</code>, femahoy {esc(version)}.</p>
<table>
<thead><tr><th>Case</th><th>Result</th><th class=n>Report</th></tr></thead>
<tbody>
{chr(10).join(rows)}
</tbody>
</table>
</main>
</body>
</html>
""")


def main(args):
    all_cases = sorted(p.name for p in CASES.iterdir() if (p / "check.toml").is_file())
    cases = args or all_cases
    for c in cases:
        if c not in all_cases:
            sys.exit(f"validation-reports: no case {c} in {CASES}")
    if not FH.is_file():
        sys.exit(f"validation-reports: no {FH}. Run cargo build --release -p fh, or set FH_BIN")
    if not (VIEWER / VIEWER_FILES[1]).is_file():
        sys.exit(f"validation-reports: no viewer in {VIEWER}. Run scripts/build-viewer.sh, or set FH_VIEWER")
    version = subprocess.run(["git", "-C", ROOT, "describe", "--always", "--dirty"],
                             capture_output=True, text=True).stdout.strip()
    OUT.mkdir(exist_ok=True)
    write_viewer()
    outcomes = read_outcomes()
    tmp = Path(tempfile.mkdtemp(prefix="fh-reports-"))
    env = dict(os.environ, FH_STATE=str(tmp / "state"))
    try:
        for case in cases:
            print(f"{case}: fh validate", flush=True)
            report = OUT / f"{case}.html"
            status, rows = validate(case, tmp, env)
            if status.startswith("cannot run"):
                report.unlink(missing_ok=True)
                why = re.sub(r"^cannot run \S+: ", "", status.split(":", 1)[1].strip())
                outcomes[case] = ["cannot run", why, ""]
                print(f"{case}: {status}", flush=True)
                continue
            try:
                runs, curves, nofield = solve(case, tmp, env)
            except RuntimeError as e:
                report.unlink(missing_ok=True)
                outcomes[case] = ["fail", str(e).replace("\n", " "), ""]
                print(f"{case}: {e}", flush=True)
                continue
            page = tmp / f"{case}.text.html"
            page.write_text(text(case, status, rows, runs, nofield, version))
            args = [a for run, _ in runs for a in ("--run", run)] + [a for c in curves for a in ("--curve", c)]
            if runs:
                args += ["--viewer", "viewer.js"]
            r = fh("report", page, "-o", report, *args, env=env)
            if r.returncode == 0 and runs and report.stat().st_size > SMALL:
                page.write_text(text(case, status, rows, runs, nofield, version, last_step=True))
                r = fh("report", page, "-o", report, *args, "--last-step", env=env)
            if r.returncode != 0:
                report.unlink(missing_ok=True)
                outcomes[case] = ["fail", r.stderr.strip(), ""]
                print(f"{case}: {r.stderr.strip()}", flush=True)
                continue
            result = status.split(":", 1)
            detail = result[1].strip() if len(result) > 1 else ""
            size = report.stat().st_size
            print(f"{case}: {result[0]}, {size} bytes", flush=True)
            if size > LIMIT:
                report.unlink()
                pending = f"Report pending: {size / 1e6:.1f} MB, above the {LIMIT / 1e6:.0f} MB limit."
                outcomes[case] = [result[0], f"{pending} {detail}".strip(), ""]
                continue
            outcomes[case] = [result[0], detail, str(size)]
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
        write_outcomes(outcomes)
        write_index(outcomes, version)


if __name__ == "__main__":
    main(sys.argv[1:])
