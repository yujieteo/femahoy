#!/usr/bin/env python3
"""Write 1 report of `fh report` for each validation case to validation/reports/.

    scripts/validation-reports.py [CASE]...

CASE is the folder name of a case in validation/cases. With no CASE, the script does all the
cases. For each case:

1. `fh validate` solves the case and gives the computed value of each check.
2. `fh run` solves each input of the case again, for the 3D view of the report. A fit case gets
   the plot of its fit (`fh fit -o`) in place of a run. curves.py in the case folder, if there
   is one, writes curve files from the outputs of the runs:
   `python3 curves.py OUT INPUT=FOLDER...`, with the output folder of the run of each input.
   Each curve file (a CSV file with a `# y:` line) in the case folder, in the output of a run
   or in OUT is a plot of the report.
3. `fh report --viewer viewer.js` writes validation/reports/CASE.html: the story of the case,
   then a table of the checks, then derivation.md. The story is story.html in the case folder:
   the conclusion of the case, with the controls that open the runs and the marks of the plots
   (see "The story of a report" in crates/fh/src/daemon/agent.md). Its `data-run` names an
   input of the case, for example `data-run="d10"`, and the script gives the name of its run.
   A case that does not pass shows the table of the checks before the story. A case with no
   story.html gets a link to each run. The story chooses its own report controls with
   `<div data-report-controls="...">` (agent.md). In the story,
   `<!--hand-check CHECK UNIT-->...<!--/hand-check-->` puts its formula and scope beside the
   result of the check CHECK of check.toml. UNIT comes from the case. The table gives the
   difference in that unit, and "Not specified" for the unit of each other check.

A case that cannot run (its solver is not installed on this computer) gets a report of the text
only: the checks of check.toml and derivation.md. The report and the index name the solver and
how to install it. A case that fails gets its report, which shows the failure. An
input that writes no field (a CalculiX input with no *NODE FILE and no *EL FILE) has no 3D view.
A report above 5 MB is made again with only the steps that the story of each run opens
(`fh report --one-step`).
A report above 10 MB is not kept: the index names its size.

The reports share 1 copy of the wasm viewer, validation/reports/viewer.js. A report opens with a
double-click when viewer.js is beside it. outcomes.tsv keeps the result of each case and the
version of femahoy that made its report, so a run for some cases keeps the lines of the other
cases. index.html links all the reports and shows the version of each.

Needs `fh` (target/release/fh, or $FH_BIN) and the wasm viewer (target/release/viewer, or
$FH_VIEWER, from scripts/build-viewer.sh). The runs go to a temporary state folder.
Only the Python standard library.
"""

import base64
import html
import json
import os
import re
import runpy
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
FH = Path(os.environ.get("FH_BIN") or ROOT / "target/release/fh")
VIEWER = Path(os.environ.get("FH_VIEWER") or ROOT / "target/release/viewer")
VIEWER_FILES = ["fh_viewer.js", "fh_viewer_bg.wasm"]
# A report above SMALL bytes keeps only the steps that the story of each run opens. The
# repository keeps no report above LIMIT bytes.
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


HOME = re.compile(re.escape(str(Path.home())) + r"(?![\w/])")
ABSOLUTE = re.compile(r"(?<![\w:/.])/(?:[^\s/]+/)+([^\s/]+)")


def local(text):
    """text as 1 line, with the home folder as ~ and each other absolute path as its last part.
    The output of fh goes into the reports through this function: the paths of this computer are
    wrong for each other reader, and a line break or a tab breaks outcomes.tsv."""
    return " ".join(ABSOLUTE.sub(r"\1", HOME.sub("~", text)).split())


def fh(*args, env):
    return subprocess.run([str(FH), *map(str, args)], env=env, capture_output=True, text=True)


def not_installed(status):
    """The status "not run: ..." of a case whose solver is not installed, or None."""
    hint = re.search(r"Install .*?(?=\. The full output|\.?$)", status)
    if not status.startswith("cannot run") or not hint:
        return None
    exe = re.search(r"cannot start (\S+):", status)
    solver = exe[1] if exe and not exe[1].startswith("python") else hint[0].split()[1]
    return f"not run: {solver} is not installed on the machine that made this report. {hint[0]}."


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
    return local(status), rows


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
    runs, curves, nofield, folders = [], curve_files((CASES / case).glob("*.csv")), [], []
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
        folder = Path(env["FH_STATE"]) / "runs" / m[1] / "out"
        folders.append(f"{name}={folder}")
        if "has no field to show" in r.stdout + r.stderr:
            nofield.append(name)
            continue
        runs.append((m[1], name))
        curves += curve_files(folder.rglob("*.csv"))
    script = CASES / case / "curves.py"
    if script.is_file():
        out = tmp / f"curves-{case}"
        out.mkdir()
        r = subprocess.run([sys.executable, "-I", str(script), str(out), *folders], capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(f"curves.py: {r.stderr.strip()}")
        curves += curve_files(out.glob("*.csv"))
    return runs, curves, nofield


def story(case, runs):
    """story.html of the case with the name of the run of each input, or None."""
    path = CASES / case / "story.html"
    if not path.is_file():
        return None
    names = {name: run for run, name in runs}

    def run(m):
        if m[1] not in names:
            raise RuntimeError(f"story.html: no run of the input {m[1]}")
        return f'data-run="{names[m[1]]}"'

    return re.sub(r'data-run="([^"]*)"', run, path.read_text())


# A story opts into a hand check with an explicit check name and unit. Never infer units.
HAND_CHECK = re.compile(r'<!--hand-check ([^ ]+) ([^>]+)-->(.*?)<!--/hand-check-->', re.S)


def difference(computed, reference, unit):
    delta = computed - reference
    absolute = f"{delta:+.6g}" + (f" {unit}" if unit else "")
    return absolute + (f" ({100 * delta / abs(reference):+.6g} %)" if reference else " (absolute)")


def text(case, status, rows, runs, nofield, version, one_step=False):
    checks = tomllib.loads((CASES / case / "check.toml").read_text())
    rel, abs_ = checks.get("rel_tol", 0), checks.get("abs_tol", 0)
    result = status.split(":", 1)[0]
    rule = (f"A check passes when |computed - reference| &le; rel_tol &middot; |reference| + abs_tol, "
            f"with rel_tol = {rel} and abs_tol = {abs_} (<code>check.toml</code>).")
    told = story(case, runs)
    hand_checks = {m[1]: (m[2], m[3]) for m in HAND_CHECK.finditer(told or "")}
    if told:
        told = HAND_CHECK.sub("", told)
    columns = ["Check", "Unit", "Computed", "Reference", "Difference", "Tolerance", "Margin"]
    if result == "not run":
        notice, _, hint = status.partition(". ")
        lead = f"<strong>Not run{esc(notice[len(result):])}.</strong> {esc(hint)} {rule}"
        columns = ["Check", "Unit", "Reference", "Tolerance"]
        rows = [[k, v["value"], rel * abs(v["value"]) + abs_] for k, v in checks.items() if isinstance(v, dict)]
    else:
        lead = f"<strong>{esc(result.capitalize())}.</strong> {rule} <code>fh validate</code> gives the computed values."
    body = ["<h2>Result</h2>", f"<p>{lead}</p>"]
    if result not in ("pass", "not run"):
        body.append(f"<pre><code>{esc(status)}</code></pre>")
    if rows:
        head = "".join(f'<th style="text-align:{"left" if h == "Check" else "right"}">{h}</th>' for h in columns)
        trs = []
        for row in rows:
            unit, formula = hand_checks.get(row[0], ("", ""))
            values = [f"{v:.6g}" for v in row[1:]]
            if result != "not run":
                values.insert(2, difference(row[1], row[2], unit))
            trs.append(f"<tr><td><code>{esc(row[0])}</code></td><td>{esc(unit or 'Not specified')}</td>"
                       + "".join(f'<td style="text-align:right">{esc(v)}</td>' for v in values) + "</tr>")
        body.append(f"<table><thead><tr>{head}</tr></thead><tbody>{''.join(trs)}</tbody></table>")
        for name, (unit, formula) in hand_checks.items():
            body.append(f'<section aria-label="Hand check {esc(name)}"><h3>Hand check: {esc(name)} [{esc(unit)}]</h3>{formula}</section>')
    last = " Only the steps that the text opens of each run are in this report, to keep the file small." if one_step else ""
    if runs and not told:
        links = ", ".join(f'<a data-run="{run}">{esc(name)}</a>' for run, name in runs)
        body.append(f"<p>The 3D view shows the run of each input: {links}.{last}</p>")
    elif last:
        body.append(f"<p>{last.strip()}</p>")
    if nofield:
        names = ", ".join(f"<code>{esc(n)}</code>" for n in nofield)
        body.append(f"<p>{names} writes no field (no <code>*NODE FILE</code> and no <code>*EL FILE</code>), "
                    "so this report has no 3D view of it.</p>")
    body.append(f"<p>Made by <code>scripts/validation-reports.py</code> on {date.today()}, femahoy {esc(version)}.</p>")
    if told:
        # The story gives the conclusion of a passed case. A case that does not pass shows its
        # result first.
        body = [told, *body] if result == "pass" else [*body, told]
    derivation = markdown((CASES / case / "derivation.md").read_text())
    # A case can use its existing curve script for a mesh summary from its source table.
    curves = CASES / case / "curves.py"
    if curves.is_file():
        mesh_report = runpy.run_path(str(curves)).get("mesh_report")
        if mesh_report:
            summary, _ = mesh_report()
            derivation = re.sub(r'(<h2>Mesh study</h2>.*?)(<table>.*?</table>)',
                                lambda m: m[1] + summary + '<details><summary>Full mesh table</summary>' + m[2] + '</details>',
                                derivation, count=1, flags=re.S)
    # The story and the result go after the title of derivation.md.
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
    return {f[0]: (f[1:] + [""] * 4)[:4] for f in (line.split("\t") for line in lines if line)}


def write_outcomes(outcomes):
    lines = ["case\tresult\tdetail\tbytes\tversion"]
    lines += ["\t".join([c, *outcomes[c]]) for c in sorted(outcomes)]
    (OUT / "outcomes.tsv").write_text("\n".join(lines) + "\n")


def write_index(outcomes):
    """index.html: a report of `fh report` with the text only, so that it has the look of the
    other views (the UI layer, DESIGN.md section 8)."""
    rows = []
    for case in sorted(p.name for p in CASES.iterdir() if (p / "check.toml").is_file()):
        result, detail, size, version = outcomes.get(case, ["not run", "", "", ""])
        name = f'<a href="{case}.html">{case}</a>' if (OUT / f"{case}.html").is_file() else case
        size = f"{int(size) / 1e6:.2f} MB" if size else ""
        note = f"<br><small>{esc(detail)}</small>" if detail else ""
        rows.append(f"<tr><td>{name}</td><td>{esc(result)}{note}</td><td>{size}</td><td>{esc(version)}</td></tr>")
    passed = sum(1 for o in outcomes.values() if o[0] == "pass")
    with tempfile.TemporaryDirectory(prefix="fh-index-") as tmp:
        text = Path(tmp) / "index.html"
        text.write_text(f"""<h1>Validation reports</h1>
<p>1 report for each validation case of femahoy: the checks, the derivation and a 3D view of the runs. {passed} of {len(rows)} cases pass. Made by <code>scripts/validation-reports.py</code>.</p>
<table>
<thead><tr><th>Case</th><th>Result</th><th>Report</th><th>Version</th></tr></thead>
<tbody>
{chr(10).join(rows)}
</tbody>
</table>
""")
        r = fh("report", text, "-o", OUT / "index.html", env=dict(os.environ, FH_STATE=str(Path(tmp) / "state")))
        return r.stderr.strip() if r.returncode else None


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
    git = lambda *a: subprocess.run(["git", "-C", ROOT, *a], capture_output=True, text=True).stdout.strip()
    # A change in validation/reports, the output of this script, does not make the code dirty.
    version = git("rev-parse", "--short", "HEAD")
    if git("status", "--porcelain", "--", ".", ":!validation/reports"):
        version += "-dirty"
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
            page = tmp / f"{case}.text.html"
            if missing := not_installed(status):
                page.write_text(text(case, missing, [], [], [], version))
                r = fh("report", page, "-o", report, env=env)
                print(f"{case}: {missing}", flush=True)
                if r.returncode != 0:
                    report.unlink(missing_ok=True)
                    outcomes[case] = ["fail", r.stderr.strip(), "", version]
                    continue
                outcomes[case] = [*missing.split(": ", 1), str(report.stat().st_size), version]
                continue
            try:
                runs, curves, nofield = solve(case, tmp, env)
                page.write_text(text(case, status, rows, runs, nofield, version))
            except RuntimeError as e:
                report.unlink(missing_ok=True)
                outcomes[case] = ["fail", str(e), "", version]
                print(f"{case}: {e}", flush=True)
                continue
            args = [a for run, _ in runs for a in ("--run", run)] + [a for c in curves for a in ("--curve", c)]
            if runs:
                args += ["--viewer", "viewer.js"]
            r = fh("report", page, "-o", report, *args, env=env)
            if r.returncode == 0 and runs and report.stat().st_size > SMALL:
                page.write_text(text(case, status, rows, runs, nofield, version, one_step=True))
                r = fh("report", page, "-o", report, *args, "--one-step", env=env)
            if r.returncode != 0:
                report.unlink(missing_ok=True)
                outcomes[case] = ["fail", r.stderr.strip(), "", version]
                print(f"{case}: {r.stderr.strip()}", flush=True)
                continue
            result = status.split(":", 1)
            detail = result[1].strip() if len(result) > 1 else ""
            size = report.stat().st_size
            print(f"{case}: {result[0]}, {size} bytes", flush=True)
            if size > LIMIT:
                report.unlink()
                pending = f"Report pending: {size / 1e6:.1f} MB, above the {LIMIT / 1e6:.0f} MB limit."
                outcomes[case] = [result[0], f"{pending} {detail}".strip(), "", version]
                continue
            outcomes[case] = [result[0], detail, str(size), version]
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
        outcomes = {c: [local(f) for f in o] for c, o in outcomes.items()}
        write_outcomes(outcomes)
        error = write_index(outcomes)
    if error:
        sys.exit(f"validation-reports: fh report index.html: {error}")


if __name__ == "__main__":
    main(sys.argv[1:])
