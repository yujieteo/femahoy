//! `fh report`: write the text of the agent, the wasm viewer, the results of some runs and the
//! plots of some curves into 1 HTML file. The person opens the file with a double-click
//! (`file://`). It needs no daemon and no network.
//!
//! The file is the template `report.html` with 3 parts put in it:
//!
//! - The text, an HTML file that the agent writes, with an x-y plot of each curve file (see
//!   [`crate::curve`]) after it. The template shows it beside the viewer.
//! - The viewer files of `fh serve` (`fh_viewer.js` and `fh_viewer_bg.wasm`), so the report
//!   uses the same viewer. With `--viewer FILE`, the report has no viewer files: it loads the
//!   script FILE, a path from the folder of the report. FILE sets `window.fhViewer` to an object
//!   with the gzip of each viewer file in base64, under its daemon path (for example
//!   `/viewer/fh_viewer.js`). Many reports in 1 folder then share 1 copy of the viewer
//!   (`scripts/validation-reports.py`).
//! - For each run, the files that the viewer fetches from the daemon: the input, `results.pvd`
//!   and each step file. Each file is a `script` element that holds its bytes in base64, with
//!   the daemon path of the file in `data-path`. A small script in the template answers the
//!   fetches of the viewer from these elements. A report with no run has no viewer.
//!
//! The template allows no script but its own (a new nonce for each report) and no network, so
//! the text of the agent cannot run a script or load a file. The fonts are in the template as
//! `data:` URLs ([`font_faces`]).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fh_core::vtu::read_pvd;

use super::http::VIEWER_FILES;
use super::serve::viewer_dir;
use super::store::Store;
use super::{default_state_dir, random_hex};
use crate::ccx::PVD_FILE;
use crate::curve::escape;
use crate::fit::read_curve;

const USAGE: &str =
    "usage: fh report TEXT.html -o REPORT.html [--run RUN]... [--curve CURVE.csv]... \
     [--viewer FILE.js]\n\
     Name at least 1 run or 1 curve. --viewer loads the viewer from the script FILE.js beside \
     the report, which sets window.fhViewer, instead of a copy in the report.";
const TEMPLATE: &str = include_str!("report.html");

struct Options {
    text: PathBuf,
    out: PathBuf,
    runs: Vec<String>,
    curves: Vec<PathBuf>,
    viewer: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let (mut text, mut out, mut runs, mut curves) = (None, None, Vec::<String>::new(), Vec::new());
    let mut viewer = None;
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
        (Some(text), Some(out)) if !runs.is_empty() || !curves.is_empty() => Ok(Options {
            text,
            out,
            runs,
            curves,
            viewer,
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

/// Add the file `bytes` that the viewer fetches from the daemon path `path` to `out`.
fn add_file(out: &mut String, path: &str, bytes: &[u8]) {
    out.push_str("<script type=\"text/plain\" data-path=\"");
    out.push_str(path);
    out.push_str("\">");
    base64(out, bytes);
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

/// The report as HTML.
fn build(o: &Options) -> Result<String, String> {
    let state = default_state_dir().ok_or("no home folder, so set FH_STATE")?;
    let mut text = String::from_utf8(read(&o.text)?)
        .map_err(|_| format!("{}: not UTF-8 text", o.text.display()))?;
    for path in &o.curves {
        text.push_str(&read_curve(path)?.figure());
    }
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
        options.push_str(&format!("<option value=\"{run}\">{label}</option>"));
    }
    let nonce = random_hex(16)?;
    let shared = match (&o.viewer, o.runs.is_empty()) {
        (Some(file), false) => format!(
            "<script src=\"{}\" nonce=\"{nonce}\"></script>",
            escape(file)
        ),
        _ => String::new(),
    };
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
    let failed = |(_, message): (u16, String)| message;
    for run in &o.runs {
        let base = format!("/api/v1/runs/{run}");
        add_file(
            &mut html,
            &format!("{base}/input"),
            &store.input_file(run).map_err(failed)?,
        );
        let pvd = store.result_file(run, PVD_FILE).map_err(failed)?;
        add_file(&mut html, &format!("{base}/files/{PVD_FILE}"), &pvd);
        let entries = read_pvd(&String::from_utf8_lossy(&pvd))
            .map_err(|e| format!("{run}: {PVD_FILE}: {e}"))?;
        for e in entries {
            let bytes = store.result_file(run, &e.file).map_err(failed)?;
            add_file(&mut html, &format!("{base}/files/{}", e.file), &bytes);
        }
    }
    html.push_str(tail);
    Ok(html)
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
            println!(
                "fh report: wrote {} ({:.1} MiB) with {}",
                o.out.display(),
                bytes as f64 / f64::from(1 << 20),
                parts.join(", ")
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("fh report: {e}");
            ExitCode::from(1)
        }
    })
}
