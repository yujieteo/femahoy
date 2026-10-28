#!/usr/bin/env python3
"""Write 1 Markdown report for each validation case to validation/reports/.

    scripts/validation-reports.py [CASE]...

CASE is the folder name of a case in validation/cases. With no CASE, the script does all the
cases. For each case:

1. `fh validate` solves the case and gives the computed value of each check.
2. `fh run` solves each input of the case again, for the plots of the report. A fit case gets
   the plot of its fit (`fh fit -o`) in place of a run. curves.py in the case folder, if there
   is one, writes curve files from the outputs of the runs:
   `python3 curves.py OUT INPUT=FOLDER...`, with the output folder of the run of each input.
   Each curve file (a CSV file with a `# y:` line) in the case folder, in the output of a run
   or in OUT is a plot of the report. mesh_trend.py in the case folder, if there is one, gives
   the mesh trend (`mesh_report()`): its panels go before the mesh table of derivation.md, and
   its plots need no run, so a case that is not run also has them.
3. The script writes validation/reports/CASE.md: the story of the case, then a table of the
   checks, then derivation.md. The story is story.md in the case folder: the conclusion of the
   case. A case that does not pass shows the table of the checks before the story. The plot of
   a curve goes in place of the mark `<!--curve NAME-->` of the story or of the mesh trend,
   where the path of the curve ends with NAME, else at the end of the report. Each plot is the
   SVG file validation/reports/CASE/NAME.svg, which `fh report` draws, and a table of its
   original values. In the story,
   `<!--hand-check CHECK UNIT-->...<!--/hand-check-->` puts its formula and scope below the
   table of the checks. UNIT comes from the case. The table gives the difference in that unit,
   and "Not specified" for the unit of each other check.

A case that cannot run (its solver is not installed on this computer) gets a report of the text
only: the checks of check.toml and derivation.md. The report and the index name the solver and
how to install it. A case that fails gets its report, which shows the failure.

outcomes.tsv keeps the result of each case and the version of femahoy that made its report, so
a run for some cases keeps the lines of the other cases. index.md links all the reports and
shows the version of each.

Needs `fh` (target/release/fh, or $FH_BIN). The runs go to a temporary state folder.
Only the Python standard library.
"""

import html
import os
import re
import runpy
import shutil
import subprocess
import sys
import tempfile
import tomllib
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "validation/cases"
OUT = ROOT / "validation/reports"
FH = Path(os.environ.get("FH_BIN") or ROOT / "target/release/fh")


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
    """The curve files of the runs of the case."""
    curves, folders = curve_files((CASES / case).glob("*.csv")), []
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
        curves += curve_files(folder.rglob("*.csv"))
    script = CASES / case / "curves.py"
    if script.is_file():
        out = tmp / f"curves-{case}"
        out.mkdir()
        r = subprocess.run([sys.executable, "-I", str(script), str(out), *folders], capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(f"curves.py: {r.stderr.strip()}")
        curves += curve_files(out.glob("*.csv"))
    return curves + mesh_curves(case, tmp)


def mesh_report(case):
    """The mesh trend of mesh_trend.py in the case folder: the Markdown panels and the curve
    files by name. It needs no run. ("", {}) if the case has no mesh_trend.py."""
    script = CASES / case / "mesh_trend.py"
    return runpy.run_path(str(script))["mesh_report"]() if script.is_file() else ("", {})


def mesh_curves(case, tmp):
    """The curve files of the mesh trend of the case, in tmp."""
    out = tmp / f"mesh-{case}"
    out.mkdir(exist_ok=True)
    for name, csv in mesh_report(case)[1].items():
        (out / name).write_text(csv)
    return sorted(out.glob("*.csv"))


def story(case):
    """story.md of the case, or None."""
    path = CASES / case / "story.md"
    return path.read_text().strip() if path.is_file() else None


# A story opts into a hand check with an explicit check name and unit. Never infer units.
HAND_CHECK = re.compile(r'<!--hand-check ([^ ]+) ([^>]+)-->(.*?)<!--/hand-check-->', re.S)


def difference(computed, reference, unit):
    delta = computed - reference
    absolute = f"{delta:+.6g}" + (f" {unit}" if unit else "")
    return absolute + (f" ({100 * delta / abs(reference):+.6g} %)" if reference else " (absolute)")


def table(head, rows, right):
    """A Markdown table. The columns from `right` on are right-aligned."""
    align = ["---" if i < right else "---:" for i in range(len(head))]
    lines = [head, align, *rows]
    return "\n".join("| " + " | ".join(str(c).replace("|", "\\|") for c in line) + " |" for line in lines)


def text(case, status, rows, version):
    """The Markdown of the report, with a mark `<!--curve NAME-->` for each plot."""
    checks = tomllib.loads((CASES / case / "check.toml").read_text())
    rel, abs_ = checks.get("rel_tol", 0), checks.get("abs_tol", 0)
    result = status.split(":", 1)[0]
    rule = (f"A check passes when |computed - reference| ≤ rel_tol · |reference| + abs_tol, "
            f"with rel_tol = {rel} and abs_tol = {abs_} (`check.toml`).")
    told = story(case)
    hand_checks = {m[1]: (m[2], m[3].strip()) for m in HAND_CHECK.finditer(told or "")}
    if told:
        told = re.sub(r"\n{3,}", "\n\n", HAND_CHECK.sub("", told)).strip()
    columns = ["Check", "Unit", "Computed", "Reference", "Difference", "Tolerance", "Margin"]
    if result == "not run":
        notice, _, hint = status.partition(". ")
        lead = f"**Not run{notice[len(result):]}.** {hint} {rule}"
        columns = ["Check", "Unit", "Reference", "Tolerance"]
        rows = [[k, v["value"], rel * abs(v["value"]) + abs_] for k, v in checks.items() if isinstance(v, dict)]
    else:
        lead = f"**{result.capitalize()}.** {rule} `fh validate` gives the computed values."
    body = ["## Result", lead]
    if result not in ("pass", "not run"):
        body.append(f"```\n{status}\n```")
    if rows:
        cells = []
        for row in rows:
            unit, _ = hand_checks.get(row[0], ("", ""))
            values = [f"{v:.6g}" for v in row[1:]]
            if result != "not run":
                values.insert(2, difference(row[1], row[2], unit))
            cells.append([f"`{row[0]}`", unit or "Not specified", *values])
        body.append(table(columns, cells, 1))
        for name, (unit, formula) in hand_checks.items():
            body.append(f"### Hand check: {name} [{unit}]\n\n{formula}")
    body.append(f"Made by `scripts/validation-reports.py` on {date.today()}, femahoy {version}.")
    if told:
        # The story gives the conclusion of a passed case. A case that does not pass shows its
        # result first.
        body = [told, *body] if result == "pass" else [*body, told]
    derivation = (CASES / case / "derivation.md").read_text().strip()
    # The mesh trend of a case goes before the full mesh table of derivation.md.
    summary, _ = mesh_report(case)
    if summary:
        derivation = re.sub(r"(^## Mesh study\n.*?\n)((?:\|[^\n]*\n)+)",
                            lambda m: m[1] + summary + "\n\n<details><summary>Full mesh table</summary>\n\n"
                            + m[2] + "\n</details>\n",
                            derivation + "\n", count=1, flags=re.S | re.M).strip()
    # The story and the result go after the title of derivation.md.
    title, _, rest = derivation.partition("\n")
    return "\n\n".join([title, *body, rest.strip()]) + "\n"


# The plots.


# The light colours of the UI layer (crates/viewer/src/ui.rs): the series --s1 to --s8, the
# side panel, the line, the text and the muted text.
SERIES = ["#2a6fc4", "#c4613f", "#138a5f", "#a8740a", "#c2457a", "#2f7d1f", "#6250d6", "#c43d3c"]
SIDE, LINE, TEXT, MUTED = "#efebe2", "#d8d2c4", "#1c1b18", "#5c5950"
STYLE = (f"text {{ fill: {MUTED}; }} .label {{ fill: {TEXT}; }} .grid {{ stroke: {LINE}; }} "
         f".frame {{ fill: none; stroke: {MUTED}; }} polyline {{ fill: none; stroke-width: 2; stroke-linejoin: round; }} "
         f"circle {{ stroke: {SIDE}; stroke-width: 2; }} .sample-cursor {{ display: none; }}")


def svg(figure):
    """The title, the SVG file and the lines of text of 1 plot that `fh report` drew: a
    `figure class="curve"` with an inline SVG, a legend and notes (crates/fh/src/curve.rs)."""
    title = html.unescape(re.search(r"<figcaption>(.*?)</figcaption>", figure)[1])
    inner = re.search(r"<svg viewBox=\"0 0 (\d+) (\d+)\"[^>]*>(.*?)</svg>", figure, re.S)
    width, height = int(inner[1]), int(inner[2])
    plot = re.sub(r"var\(--s(\d)\)", lambda m: SERIES[int(m[1]) - 1], inner[3])
    # The legend goes below the plot, 1 series on each line.
    legend = re.findall(r"<li><i class=\"(\w+)\" style=\"background:var\(--s(\d)\)\"></i>(.*?)</li>", figure)
    keys = []
    for i, (kind, s, name) in enumerate(legend):
        y = height + 18 * i + 12
        colour = SERIES[int(s) - 1]
        mark = (f'<circle cx="84" cy="{y - 4}" r="4" fill="{colour}"/>' if kind == "point"
                else f'<line x1="72" y1="{y - 4}" x2="96" y2="{y - 4}" stroke="{colour}" stroke-width="2"/>')
        keys.append(f'{mark}<text class="label" x="104" y="{y}">{name}</text>')
    height += 18 * len(legend) + (8 if legend else 0)
    file = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" '
            f'font-family="sans-serif" font-size="12"><title>{html.escape(title)}</title><style>{STYLE}</style>'
            f'<rect width="100%" height="100%" fill="{SIDE}"/>{plot}{"".join(keys)}</svg>\n')
    tail = figure[figure.index("</svg>"):]
    lines = [html.unescape(p) for p in re.findall(r"<p>(.*?)</p>", tail[tail.find("</div></div>"):])]
    return title, file, lines, values(tail)


def values(figure):
    """The table of the original values of a plot (the "Show values" table of curve.rs) as a
    Markdown table in a `details` block."""
    found = re.search(r"<table><caption>(.*?)</caption>(.*?)</table>", figure, re.S)
    cell = lambda c: html.unescape(re.sub(r"<[^>]+>", "", c))
    rows = [[cell(c) for c in re.findall(r"<t[hd][^>]*>(.*?)</t[hd]>", tr)] for tr in re.findall(r"<tr[^>]*>(.*?)</tr>", found[2], re.S)]
    return (f"<details><summary>Values of the plot</summary>\n\n{cell(found[1])}\n\n"
            + table(rows[0], rows[1:], 0) + "\n\n</details>")


def plots(case, page, curves, env, tmp):
    """page with each mark `<!--curve NAME-->` as a plot, and each other curve at the end.
    Writes the SVG files to validation/reports/CASE/."""
    folder = OUT / case
    shutil.rmtree(folder, ignore_errors=True)
    names = set()
    for curve in curves:
        marks = [m for m in re.findall(r"<!--curve (\S+)-->", page) if str(curve).endswith(m)]
        name = curve.stem
        while name in names:
            name += "-2"
        names.add(name)
        draft = tmp / f"plot-{case}-{name}.html"
        draft.write_text(f"<!--curve {curve.name}-->")
        r = fh("report", draft, "-o", draft.with_suffix(".report.html"), "--curve", curve, env=env)
        if r.returncode != 0:
            raise RuntimeError(f"fh report {curve.name}: {r.stderr.strip()}")
        figure = re.search(r"<figure class=\"curve\">.*?</figure>", draft.with_suffix(".report.html").read_text(), re.S)[0]
        title, file, lines, samples = svg(figure)
        folder.mkdir(exist_ok=True)
        (folder / f"{name}.svg").write_text(file)
        plot = "\n\n".join([f"**{title}**", f"![{title}]({case}/{name}.svg)", *lines, samples])
        if marks:
            page = page.replace(f"<!--curve {marks[0]}-->", plot, 1)
        else:
            page = page.rstrip("\n") + "\n\n" + plot + "\n"
    return page


def read_outcomes():
    path = OUT / "outcomes.tsv"
    if not path.is_file():
        return {}
    lines = path.read_text().splitlines()[1:]
    return {f[0]: (f[1:] + [""] * 3)[:3] for f in (line.split("\t") for line in lines if line)}


def write_outcomes(outcomes):
    lines = ["case\tresult\tdetail\tversion"]
    lines += ["\t".join([c, *outcomes[c]]) for c in sorted(outcomes)]
    (OUT / "outcomes.tsv").write_text("\n".join(lines) + "\n")


def write_index(outcomes):
    rows = []
    for case in sorted(p.name for p in CASES.iterdir() if (p / "check.toml").is_file()):
        result, detail, version = outcomes.get(case, ["not run", "", ""])
        name = f"[{case}]({case}.md)" if (OUT / f"{case}.md").is_file() else case
        rows.append([name, result + (f": {detail}" if detail else ""), version])
    passed = sum(1 for o in outcomes.values() if o[0] == "pass")
    (OUT / "index.md").write_text(
        "# Validation reports\n\n"
        "1 report for each validation case of femahoy: the checks, the derivation and the plots of the runs. "
        f"{passed} of {len(rows)} cases pass. Made by `scripts/validation-reports.py`.\n\n"
        + table(["Case", "Result", "Version"], rows, 3) + "\n")


def main(args):
    all_cases = sorted(p.name for p in CASES.iterdir() if (p / "check.toml").is_file())
    cases = args or all_cases
    for c in cases:
        if c not in all_cases:
            sys.exit(f"validation-reports: no case {c} in {CASES}")
    if not FH.is_file():
        sys.exit(f"validation-reports: no {FH}. Run cargo build --release -p fh, or set FH_BIN")
    git = lambda *a: subprocess.run(["git", "-C", ROOT, *a], capture_output=True, text=True).stdout.strip()
    # A change in validation/reports, the output of this script, does not make the code dirty.
    version = git("rev-parse", "--short", "HEAD")
    if git("status", "--porcelain", "--", ".", ":!validation/reports"):
        version += "-dirty"
    OUT.mkdir(exist_ok=True)
    outcomes = read_outcomes()
    tmp = Path(tempfile.mkdtemp(prefix="fh-reports-"))
    env = dict(os.environ, FH_STATE=str(tmp / "state"))
    try:
        for case in cases:
            print(f"{case}: fh validate", flush=True)
            report = OUT / f"{case}.md"
            status, rows = validate(case, tmp, env)
            try:
                if missing := not_installed(status):
                    report.write_text(plots(case, text(case, missing, [], version), mesh_curves(case, tmp), env, tmp))
                    print(f"{case}: {missing}", flush=True)
                    outcomes[case] = [*missing.split(": ", 1), version]
                    continue
                report.write_text(plots(case, text(case, status, rows, version), solve(case, tmp, env), env, tmp))
            except RuntimeError as e:
                report.unlink(missing_ok=True)
                shutil.rmtree(OUT / case, ignore_errors=True)
                outcomes[case] = ["fail", str(e), version]
                print(f"{case}: {e}", flush=True)
                continue
            result = status.split(":", 1)
            print(f"{case}: {result[0]}", flush=True)
            outcomes[case] = [result[0], result[1].strip() if len(result) > 1 else "", version]
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
        outcomes = {c: [local(f) for f in o] for c, o in outcomes.items()}
        write_outcomes(outcomes)
        write_index(outcomes)


if __name__ == "__main__":
    main(sys.argv[1:])
