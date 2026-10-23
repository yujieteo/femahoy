//! `fh report`: write the text of the agent, the wasm viewer, the results of some runs and the
//! plots of some curves into 1 HTML file. The person opens the file with a double-click
//! (`file://`). It needs no daemon and no network.
//!
//! The file is the template `report.html` with 3 parts put in it:
//!
//! - The text, an HTML file that the agent writes, with an x-y plot of each curve file (see
//!   [`crate::curve`]). The plot of a curve goes in place of the mark `<!--curve NAME-->` of the
//!   text, where the path of the curve ends with NAME, else after the text. The template shows
//!   the text beside the viewer. The template script makes the controls that the agent writes
//!   in the text work: an element with `data-run` opens that run with the view of its other
//!   `data-` attributes (`report.html`). Each `data-run` must name a run of the report, each
//!   name of its `data-fields` a field of the run, and its `data-step` a step of the run
//!   ([`check`]).
//! - The viewer files of `fh serve` (`fh_viewer.js` and `fh_viewer_bg.wasm`), so the report
//!   uses the same viewer. With `--viewer FILE`, the report has no viewer files: it loads the
//!   script FILE, a path from the folder of the report. FILE sets `window.fhViewer` to an object
//!   with each viewer file as raw deflate in base64, under its daemon path (for example
//!   `/viewer/fh_viewer.js`). Many reports in 1 folder then share 1 copy of the viewer
//!   (`scripts/validation-reports.py`).
//! - For each run, the files that the viewer fetches from the daemon: the input, `results.pvd`
//!   and each step file. With `--one-step`, only the steps of each run that the elements of the
//!   text for the run open: the step of each `data-step`, and the last step for an element with
//!   no `data-step` or a run with no element. Each `data-step` then names its step in the kept
//!   steps ([`kept_steps`]). Each file is a `script` element that holds its bytes as raw
//!   deflate in base64, with the daemon path of the file in `data-path`. A small script in the
//!   template answers the fetches of the viewer from these elements. A report with no run has
//!   no viewer.
//!
//! The template allows no script but its own (a new nonce for each report) and no network, so
//! the text of the agent cannot run a script or load a file. The fonts are in the template as
//! `data:` URLs ([`font_faces`]).

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fh_core::vtu::{read_pvd, PvdEntry};
use miniz_oxide::deflate::compress_to_vec;

use super::http::VIEWER_FILES;
use super::serve::viewer_dir;
use super::store::Store;
use super::{default_state_dir, random_hex};
use crate::ccx::PVD_FILE;
use crate::curve::escape;
use crate::fit::read_curve;

const USAGE: &str =
    "usage: fh report TEXT.html -o REPORT.html [--run RUN]... [--curve CURVE.csv]... \
     [--viewer FILE.js] [--one-step]\n\
     A report with no run and no curve is the text only. The plot of a curve goes in place of \
     the mark <!--curve NAME--> of the text, where the path of the curve ends with NAME, else \
     after the text. --viewer loads the viewer from the script FILE.js beside the report, \
     which sets window.fhViewer, instead of a copy in the report. Each data-run of the text \
     must name a --run, each name of its data-fields a field of the run, and its data-step a \
     step of the run. --one-step keeps only the steps of each run that the text opens, for a \
     smaller file: the step of each data-step, and the last step for an element with no \
     data-step or a run with no element.";
const TEMPLATE: &str = include_str!("report.html");

struct Options {
    text: PathBuf,
    out: PathBuf,
    runs: Vec<String>,
    curves: Vec<PathBuf>,
    viewer: Option<String>,
    one_step: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let (mut text, mut out, mut runs, mut curves) = (None, None, Vec::<String>::new(), Vec::new());
    let (mut viewer, mut one_step) = (None, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |what: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs {what}\n{USAGE}"))
        };
        match arg.as_str() {
            "-o" => out = Some(PathBuf::from(value("a file")?)),
            "--curve" => curves.push(PathBuf::from(value("a curve file")?)),
            "--viewer" => viewer = Some(value("a script file")?),
            "--one-step" => one_step = true,
            "--run" => {
                let run = value("a run")?;
                if !runs.contains(&run) {
                    runs.push(run);
                }
            }
            s if s.starts_with('-') => return Err(format!("unknown option: {s}\n{USAGE}")),
            s if text.is_none() => text = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument: {s}\n{USAGE}")),
        }
    }
    match (text, out) {
        (Some(text), Some(out)) => Ok(Options {
            text,
            out,
            runs,
            curves,
            viewer,
            one_step,
        }),
        _ => Err(USAGE.into()),
    }
}

/// Add `bytes` in base64 to `out`.
fn base64(out: &mut String, bytes: &[u8]) {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    out.reserve(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                DIGITS[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
}

/// The `@font-face` rules of the fonts of the site style (`fonts/`, SIL Open Font License 1.1):
/// Source Serif 4 for the titles and DM Sans for the text. The fonts are `data:` URLs, so the
/// page of `fh serve` and a report load no font file.
pub(super) fn font_faces() -> String {
    let mut css = String::new();
    for (family, weight, bytes) in [
        (
            "Source Serif 4",
            "400",
            &include_bytes!("fonts/source-serif-4.woff2")[..],
        ),
        (
            "DM Sans",
            "100 1000",
            &include_bytes!("fonts/dm-sans.woff2")[..],
        ),
    ] {
        css.push_str(&format!(
            "@font-face {{ font-family: \"{family}\"; font-weight: {weight}; src: \
             url(data:font/woff2;base64,"
        ));
        base64(&mut css, bytes);
        css.push_str(") format(\"woff2\"); }\n");
    }
    css
}

/// Add the file `bytes` that the viewer fetches from the daemon path `path` to `out`, as raw
/// deflate (the `deflate-raw` of `DecompressionStream`) in base64.
fn add_file(out: &mut String, path: &str, bytes: &[u8]) {
    out.push_str("<script type=\"text/plain\" data-path=\"");
    out.push_str(path);
    out.push_str("\">");
    base64(out, &compress_to_vec(bytes, 9));
    out.push_str("</script>\n");
}

/// The folder and the name of the input file of a run, for the list of runs.
fn short(source: &str) -> String {
    let mut parts: Vec<&str> = source.rsplit('/').take(2).collect();
    parts.reverse();
    parts.join("/")
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The text with the plot of each curve of `paths` in place of its mark `<!--curve NAME-->`,
/// where the path of the curve ends with NAME. The plots with no mark go after the text.
fn place_curves(mut text: String, paths: &[PathBuf]) -> Result<String, String> {
    let mut after = String::new();
    for path in paths {
        let figure = read_curve(path)?.figure();
        let mark = text.match_indices("<!--curve ").find_map(|(at, _)| {
            let end = at + text[at..].find("-->")? + 3;
            let name = text[at + 10..end - 3].trim();
            (!name.is_empty() && path.ends_with(name)).then_some(at..end)
        });
        match mark {
            Some(range) => text.replace_range(range, &figure),
            None => after.push_str(&figure),
        }
    }
    text.push_str(&after);
    Ok(text)
}

/// The place in `tag` of the value of its attribute `name="VALUE"`.
fn attribute(tag: &str, name: &str) -> Option<Range<usize>> {
    let pattern = format!("{name}=\"");
    let (at, _) = tag
        .match_indices(&pattern)
        .find(|(at, _)| tag[..*at].ends_with(char::is_whitespace))?;
    let start = at + pattern.len();
    Some(start..start + tag[start..].find('"')?)
}

/// An element of the text that opens a run: its run, and the place in the text of the values
/// of its `data-fields` and its `data-step`.
struct Control<'a> {
    run: &'a str,
    fields: Option<Range<usize>>,
    step: Option<Range<usize>>,
}

/// Each element of the text with `data-run`.
fn controls(text: &str) -> Vec<Control<'_>> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices("data-run=\"") {
        let Some(start) = text[..at]
            .rfind(['<', '>'])
            .filter(|s| text[*s..].starts_with('<'))
        else {
            continue;
        };
        let Some(end) = text[at..].find('>').map(|n| at + n) else {
            continue;
        };
        let value =
            |name| attribute(&text[start..end], name).map(|r| start + r.start..start + r.end);
        let (Some(run), fields, step) =
            (value("data-run"), value("data-fields"), value("data-step"))
        else {
            continue;
        };
        found.push(Control {
            run: &text[run],
            fields,
            step,
        });
    }
    found
}

/// An error when a control of `run` does not fit the run: a `data-step` that is not a step of
/// its `steps` steps, or a name of its `data-fields` that is not a field of the run. The fields
/// come from `results` only when a control names a field.
fn check(
    text: &str,
    run: &str,
    controls: &[&Control],
    steps: usize,
    results: impl FnOnce() -> Result<fh_viewer::results::Results, String>,
) -> Result<(), String> {
    for c in controls {
        if let Some(range) = &c.step {
            let value = &text[range.clone()];
            if !value
                .parse::<usize>()
                .is_ok_and(|n| (1..=steps).contains(&n))
            {
                return Err(format!(
                    "{run}: data-step=\"{value}\": the run has {steps} steps"
                ));
            }
        }
    }
    let named: Vec<&str> = controls
        .iter()
        .filter_map(|c| c.fields.clone())
        .flat_map(|range| text[range].split(','))
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect();
    if named.is_empty() {
        return Ok(());
    }
    let names = fh_viewer::view::field_names(&results()?);
    match named.iter().find(|f| !names.iter().any(|n| n == *f)) {
        Some(f) => Err(format!(
            "{run}: data-fields: the run has no field \"{f}\". Its fields are {}",
            names.join(", ")
        )),
        None => Ok(()),
    }
}

/// The steps (from 0) of a run of `steps` steps that its controls open: the step of each
/// `data-step`, and the last step for a control with no `data-step` or a run with no control.
/// In order, with no step 2 times.
fn kept_steps(text: &str, controls: &[&Control], steps: usize) -> Vec<usize> {
    let last = steps.saturating_sub(1);
    let mut kept: Vec<usize> = controls
        .iter()
        .map(|c| match &c.step {
            Some(range) => text[range.clone()].parse::<usize>().map_or(last, |n| n - 1),
            None => last,
        })
        .collect();
    if kept.is_empty() {
        kept.push(last);
    }
    kept.sort_unstable();
    kept.dedup();
    kept
}

/// The report as HTML.
fn build(o: &Options) -> Result<String, String> {
    let state = default_state_dir().ok_or("no home folder, so set FH_STATE")?;
    let text = String::from_utf8(read(&o.text)?)
        .map_err(|_| format!("{}: not UTF-8 text", o.text.display()))?;
    let mut text = place_curves(text, &o.curves)?;
    let store = Store::open(&state)?;
    let found = store.runs().map_err(|(_, m)| m)?;
    let mut options = String::new();
    for run in &o.runs {
        let (_, source) = found.iter().find(|(r, _)| r == run).ok_or_else(|| {
            format!(
                "{run} has no results in {}. Name a run of `fh run`",
                state.display()
            )
        })?;
        // A run in `found` is `run-` and digits, so it needs no escape.
        let label = escape(&format!("{run} · {}", short(source)));
        options.push_str(&format!(
            "<option value=\"{run}\" data-run=\"{run}\">{label}</option>"
        ));
    }
    let nonce = random_hex(16)?;
    let shared = match (&o.viewer, o.runs.is_empty()) {
        (Some(file), false) => format!(
            "<script src=\"{}\" nonce=\"{nonce}\"></script>",
            escape(file)
        ),
        _ => String::new(),
    };
    let found_controls = controls(&text);
    if let Some(c) = found_controls
        .iter()
        .find(|c| !o.runs.iter().any(|r| r == c.run))
    {
        return Err(format!(
            "data-run=\"{}\": the report has no such run. Name it with --run",
            c.run
        ));
    }
    let failed = |(_, message): (u16, String)| message;
    let mut files = Vec::new();
    let mut steps_of = Vec::new();
    for run in &o.runs {
        let input = store.input_file(run).map_err(failed)?;
        let pvd = String::from_utf8_lossy(&store.result_file(run, PVD_FILE).map_err(failed)?)
            .into_owned();
        let mut entries = read_pvd(&pvd).map_err(|e| format!("{run}: {PVD_FILE}: {e}"))?;
        let mine: Vec<&Control> = found_controls.iter().filter(|c| c.run == run).collect();
        check(&text, run, &mine, entries.len(), || {
            fh_viewer::results::load(PVD_FILE, &pvd, |file| {
                store.result_file(run, file).map_err(|(_, m)| m)
            })
            .map_err(|e| format!("{run}: {e}"))
        })?;
        let pvd = if o.one_step && !entries.is_empty() {
            let kept = kept_steps(&text, &mine, entries.len());
            for c in &mine {
                if let Some(range) = &c.step {
                    let n = text[range.clone()].parse::<usize>().map_or(0, |n| n - 1);
                    let at = kept.iter().position(|k| *k == n).unwrap_or(0);
                    steps_of.push((range.clone(), (at + 1).to_string()));
                }
            }
            entries = kept.into_iter().map(|k| entries[k].clone()).collect();
            keep_steps(&pvd, &entries)
        } else {
            pvd
        };
        files.push((run, input, pvd, entries));
    }
    steps_of.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, step) in steps_of {
        text.replace_range(range, &step);
    }
    let page = TEMPLATE
        .replace("NONCE", &nonce)
        .replace("/*FONTS*/", &font_faces())
        .replace("<!--VIEWER-->", &shared);
    let (head, tail) = page
        .split_once("<!--DATA-->")
        .ok_or("report.html has no data mark")?;
    // The text goes in last, so that no later step reads it.
    let mut html = head
        .replace("<!--OPTIONS-->", &options)
        .replace("<!--TEXT-->", &text);
    if !o.runs.is_empty() && o.viewer.is_none() {
        let viewer = viewer_dir().ok_or(
            "the wasm viewer is not built. Run scripts/build-viewer.sh, or set FH_VIEWER to its \
             folder",
        )?;
        for (name, _) in VIEWER_FILES {
            add_file(
                &mut html,
                &format!("/viewer/{name}"),
                &read(&viewer.join(name))?,
            );
        }
    }
    for (run, input, pvd, entries) in files {
        let base = format!("/api/v1/runs/{run}");
        add_file(&mut html, &format!("{base}/input"), &input);
        add_file(
            &mut html,
            &format!("{base}/files/{PVD_FILE}"),
            pvd.as_bytes(),
        );
        for e in entries {
            let bytes = store.result_file(run, &e.file).map_err(failed)?;
            add_file(&mut html, &format!("{base}/files/{}", e.file), &bytes);
        }
    }
    html.push_str(tail);
    Ok(html)
}

/// The PVD file `pvd` with only the data sets of `kept`.
fn keep_steps(pvd: &str, kept: &[PvdEntry]) -> String {
    let mut parts = pvd.split('<');
    let mut out = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let keep = !part.starts_with("DataSet")
            || kept
                .iter()
                .any(|e| part.contains(&format!("file=\"{}\"", e.file)));
        if keep {
            out.push('<');
            out.push_str(part);
        }
    }
    out
}

/// Run `fh report` with the arguments after `report`.
pub fn report(args: &[String]) -> Result<ExitCode, String> {
    let o = parse(args)?;
    let written = build(&o).and_then(|html| {
        fs::write(&o.out, &html).map_err(|e| format!("{}: {e}", o.out.display()))?;
        Ok(html.len())
    });
    Ok(match written {
        Ok(bytes) => {
            let curves = o.curves.iter().map(|c| c.display().to_string());
            let parts: Vec<String> = o.runs.iter().cloned().chain(curves).collect();
            let with = if parts.is_empty() {
                "the text only".to_owned()
            } else {
                parts.join(", ")
            };
            println!(
                "fh report: wrote {} ({:.1} MiB) with {with}",
                o.out.display(),
                bytes as f64 / f64::from(1 << 20),
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("fh report: {e}");
            ExitCode::from(1)
        }
    })
}
