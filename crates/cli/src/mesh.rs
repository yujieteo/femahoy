//! `fh mesh FILE.(geo|step) [options]`: run the external gmsh program and check the `.msh` file.
//! Decision D-02, see DESIGN.md: gmsh is a separate program. `fh` never links it.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const USAGE: &str = "usage: fh mesh FILE.(geo|step) [-o OUT.msh] [--order 1|2] [--size H] \
[--scale S] [--set NAME=VALUE]... [--units SI-mm|SI-m] [--timeout SECONDS] [--gmsh PATH]\n\
v1 refuses a .geo file with SystemCall, NonBlockingSystemCall, OnelabRun, Include, \
or Merge of a file that is not .msh.";

/// Tested gmsh release line, and the oldest release that `fh mesh` accepts.
const TESTED: (u32, u32) = (4, 15);
const OLDEST: (u32, u32) = (4, 8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Units {
    Mm,
    M,
}

impl Units {
    fn name(self) -> &'static str {
        match self {
            Units::Mm => "SI-mm",
            Units::M => "SI-m",
        }
    }

    /// The value of the gmsh option `Geometry.OCCTargetUnit`.
    fn gmsh_target(self) -> &'static str {
        match self {
            Units::Mm => "MM",
            Units::M => "M",
        }
    }
}

struct Options {
    input: PathBuf,
    output: PathBuf,
    order: u8,
    size: Option<f64>,
    scale: Option<f64>,
    numbers: Vec<(String, f64)>,
    units: Units,
    timeout: Duration,
    gmsh: Option<PathBuf>,
}

fn number(flag: &str, text: Option<&String>) -> Result<f64, String> {
    let value: f64 = text
        .ok_or(format!("{flag} needs a value"))?
        .parse()
        .map_err(|_| format!("{flag} needs a number"))?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(format!("{flag} needs a number above 0"))
    }
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut input: Option<PathBuf> = None;
    let mut o = Options {
        input: PathBuf::new(),
        output: PathBuf::new(),
        order: 2,
        size: None,
        scale: None,
        numbers: Vec::new(),
        units: Units::Mm,
        timeout: Duration::from_secs(300),
        gmsh: None,
    };
    let mut output: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => output = Some(it.next().ok_or("-o needs a file")?.into()),
            "--order" => {
                o.order = match it.next().map(String::as_str) {
                    Some("1") => 1,
                    Some("2") => 2,
                    _ => return Err("--order needs 1 or 2".into()),
                }
            }
            "--size" => o.size = Some(number("--size", it.next())?),
            "--scale" => o.scale = Some(number("--scale", it.next())?),
            "--set" => {
                let text = it.next().ok_or("--set needs NAME=VALUE")?;
                let (name, value) = text.split_once('=').ok_or("--set needs NAME=VALUE")?;
                let name_ok = !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
                let value: f64 = value
                    .parse()
                    .ok()
                    .filter(|v: &f64| v.is_finite())
                    .ok_or(format!("--set {name}: the value must be a number"))?;
                if !name_ok {
                    return Err(format!("--set: bad name `{name}`"));
                }
                o.numbers.push((name.to_string(), value));
            }
            "--units" => {
                o.units = match it.next().map(String::as_str) {
                    Some("SI-mm") => Units::Mm,
                    Some("SI-m") => Units::M,
                    _ => return Err("--units needs SI-mm or SI-m".into()),
                }
            }
            "--timeout" => {
                let seconds = number("--timeout", it.next())?;
                o.timeout = Duration::try_from_secs_f64(seconds)
                    .map_err(|_| "--timeout is too large".to_string())?;
            }
            "--gmsh" => o.gmsh = Some(it.next().ok_or("--gmsh needs a path")?.into()),
            s if s.starts_with('-') => return Err(format!("unknown option: {s}\n{USAGE}")),
            s => {
                if input.replace(s.into()).is_some() {
                    return Err(format!("only one input file is allowed\n{USAGE}"));
                }
            }
        }
    }
    o.input = input.ok_or(format!("no input file\n{USAGE}"))?;
    o.output = output.unwrap_or_else(|| o.input.with_extension("msh"));
    Ok(o)
}

/// A refusal of the input that does not need gmsh.
/// A `.geo` file is code. v1 refuses every command that can run another program or file:
/// `SystemCall`, `NonBlockingSystemCall`, `OnelabRun`, `Include`, and `Merge` of a non-`.msh` file.
fn check_geo(text: &str) -> Result<(), String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut merge_pending = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if c == '"' {
            let mut literal = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                literal.push(chars[i]);
                i += 1;
            }
            i += 1;
            if std::mem::take(&mut merge_pending) && !literal.to_ascii_lowercase().ends_with(".msh")
            {
                return Err(format!(
                    "the .geo file merges `{literal}`. fh mesh v1 refuses Merge of a file that is not .msh"
                ));
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            if merge_pending {
                return Err(
                    "the .geo file has a Merge that is not a plain file name. fh mesh v1 refuses it"
                        .into(),
                );
            }
            let word: String = chars[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            match word.as_str() {
                "systemcall" | "nonblockingsystemcall" | "onelabrun" | "include" => {
                    return Err(format!(
                        "the .geo file contains {word}. fh mesh v1 refuses it, \
because gmsh would run a shell command or another file"
                    ));
                }
                "merge" => merge_pending = true,
                _ => {}
            }
        } else {
            if !c.is_whitespace() && c != '(' && merge_pending {
                return Err(
                    "the .geo file has a Merge that is not a plain file name. fh mesh v1 refuses it"
                        .into(),
                );
            }
            i += 1;
        }
    }
    if merge_pending {
        return Err("the .geo file has a Merge without a file name".into());
    }
    Ok(())
}

/// The length unit that a STEP file declares. `Err` holds a refusal code and text.
fn step_unit(text: &str) -> Result<&'static str, String> {
    let upper = text.to_ascii_uppercase();
    let mut found: Vec<&'static str> = Vec::new();
    for record in upper.split(';').filter(|r| r.contains("LENGTH_UNIT()")) {
        let unit = if record.contains("CONVERSION_BASED_UNIT") {
            if record.contains("'INCH'") || record.contains("'IN'") {
                "inch"
            } else {
                "other"
            }
        } else if record.contains(".METRE.") {
            if record.contains(".MILLI.") {
                "mm"
            } else if record.contains(".CENTI.") {
                "cm"
            } else if record.contains("$,.METRE.") {
                "m"
            } else {
                "other"
            }
        } else {
            "other"
        };
        if !found.contains(&unit) {
            found.push(unit);
        }
    }
    match found.as_slice() {
        [] => Err("unit.step.missing: the STEP file has no length unit record".into()),
        [one] => Ok(one),
        _ => Err(format!(
            "unit.step.mixed: the STEP file has more than one length unit ({})",
            found.join(", ")
        )),
    }
}

fn check_step(text: &str, units: Units) -> Result<(), String> {
    let unit = step_unit(text)?;
    let wanted = match units {
        Units::Mm => "mm",
        Units::M => "m",
    };
    if unit == wanted {
        Ok(())
    } else {
        Err(format!(
            "unit.step.mismatch: the STEP file uses {unit}, but the case units are {}",
            units.name()
        ))
    }
}

fn quote_ok(path: &Path) -> Result<String, String> {
    let text = path.to_string_lossy().into_owned();
    if text.contains(['"', '\n', '\r']) {
        return Err(format!("the path has a bad character: {text}"));
    }
    Ok(text)
}

/// A gmsh program: the `--gmsh` path, `FH_GMSH`, `gmsh` on `PATH`, or the macOS app.
fn find_gmsh(option: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = option {
        return Ok(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("FH_GMSH").filter(|p| !p.is_empty()) {
        return Ok(p.into());
    }
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("gmsh")).collect())
        .unwrap_or_default();
    candidates.push("/Applications/Gmsh.app/Contents/MacOS/gmsh".into());
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        "gmsh is not installed. Install it from your package manager or from gmsh.info, \
then set FH_GMSH or use --gmsh PATH"
            .into()
    })
}

struct Finished {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

/// Run a command in `dir` with a time limit. Kill it on expiry.
/// The output goes to files, so a full pipe cannot block the child.
fn run_limited(
    gmsh: &Path,
    args: &[String],
    dir: &Path,
    limit: Duration,
) -> Result<Finished, String> {
    let open = |name: &str| std::fs::File::create(dir.join(name)).map_err(|e| e.to_string());
    let mut child = Command::new(gmsh)
        .args(args)
        .current_dir(dir)
        // gmsh reads `~/.gmsh-options` and writes into HOME. Give it an empty folder.
        .env("HOME", dir)
        .stdin(Stdio::null())
        .stdout(open("stdout.txt")?)
        .stderr(open("stderr.txt")?)
        .spawn()
        .map_err(|e| format!("cannot start gmsh ({}): {e}", gmsh.display()))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if start.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "gmsh timed out after {} seconds and was stopped",
                    limit.as_secs_f64()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let read = |name: &str| {
        String::from_utf8_lossy(&std::fs::read(dir.join(name)).unwrap_or_default()).into_owned()
    };
    Ok(Finished {
        status,
        stdout: read("stdout.txt"),
        stderr: read("stderr.txt"),
    })
}

fn version_of(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((major, minor))
}

fn lines_with(prefix: &str, texts: [&str; 2]) -> Vec<String> {
    texts
        .iter()
        .flat_map(|t| t.lines())
        .filter(|l| l.trim_start().starts_with(prefix))
        .map(|l| l.trim().to_string())
        .collect()
}

/// A unique empty folder for one run.
fn run_dir() -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("fh-mesh-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    Ok(dir)
}

/// What a run makes, or a failure text.
fn mesh(o: &Options, dir: &Path) -> Result<String, String> {
    let input = std::path::absolute(&o.input).map_err(|e| e.to_string())?;
    let output = std::path::absolute(&o.output).map_err(|e| e.to_string())?;
    let extension = input
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    let bytes = std::fs::read(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let script = match extension.as_deref() {
        Some("geo") => {
            check_geo(&text)?;
            input.clone()
        }
        Some("step" | "stp") => {
            check_step(&text, o.units)?;
            // Pin the target unit in every run. The gmsh default is a process state.
            let wrapper = dir.join("input.geo");
            let body = format!(
                "SetFactory(\"OpenCASCADE\");\nGeometry.OCCTargetUnit = \"{}\";\nMerge \"{}\";\n",
                o.units.gmsh_target(),
                quote_ok(&input)?
            );
            std::fs::write(&wrapper, body).map_err(|e| e.to_string())?;
            wrapper
        }
        _ => return Err("the input must be a .geo or a .step file".into()),
    };
    let gmsh = find_gmsh(o.gmsh.as_deref())?;

    let probe = run_limited(&gmsh, &["-version".into()], dir, Duration::from_secs(30))?;
    let version = probe.stdout.trim().to_string();
    let parsed = version_of(&version)
        .ok_or_else(|| format!("cannot read the gmsh version from `{version}`"))?;
    if parsed < OLDEST {
        return Err(format!(
            "gmsh {version} is too old, use {}.{} or newer",
            OLDEST.0, OLDEST.1
        ));
    }

    // A stale file must not pass as a new result.
    match std::fs::remove_file(&output) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("cannot replace {}: {e}", output.display()))
        }
        _ => {}
    }
    let mut args: Vec<String> = ["-3", "-nopopup", "-format", "msh41", "-v", "2"]
        .map(String::from)
        .into();
    args.extend(["-order".into(), o.order.to_string()]);
    for (flag, value) in [("-clmax", o.size), ("-clscale", o.scale)] {
        if let Some(v) = value {
            args.extend([flag.into(), v.to_string()]);
        }
    }
    for (name, value) in &o.numbers {
        args.extend(["-setnumber".into(), name.clone(), value.to_string()]);
    }
    args.extend(["-o".into(), output.to_string_lossy().into_owned()]);
    args.push(script.to_string_lossy().into_owned());
    let run = run_limited(&gmsh, &args, dir, o.timeout)?;

    let errors = lines_with("Error", [&run.stderr, &run.stdout]);
    if !run.status.success() || !errors.is_empty() {
        let mut message = match run.status.code() {
            Some(code) => format!("gmsh failed (exit code {code})"),
            None => "gmsh failed (stopped by a signal)".to_string(),
        };
        if errors.is_empty() {
            message.push_str(&format!(": {}", run.stderr.trim()));
        }
        for line in &errors {
            message.push_str(&format!("\n  {line}"));
        }
        return Err(message);
    }
    let msh = std::fs::read_to_string(&output)
        .map_err(|_| format!("gmsh wrote no mesh file: {}", output.display()))?;
    let model = fh_io::read_msh(&msh)
        .map_err(|e| format!("the mesh is not readable ({}): {e}", output.display()))?;
    if model.elements.is_empty() {
        return Err(format!(
            "gmsh wrote a mesh with no volume elements: {}",
            output.display()
        ));
    }

    let mut report = format!(
        "ok\t{}\ngmsh\t{version}\nunits\t{}\norder\t{}\nnodes\t{}\nelements\t{}\n",
        output.display(),
        o.units.name(),
        o.order,
        model.nodes.len(),
        model.elements.len()
    );
    let mut types: Vec<(&str, usize)> = Vec::new();
    for e in &model.elements {
        match types.iter_mut().find(|(t, _)| *t == e.element_type) {
            Some(entry) => entry.1 += 1,
            None => types.push((&e.element_type, 1)),
        }
    }
    for (t, n) in types {
        report.push_str(&format!("type\t{t}\t{n}\n"));
    }
    for s in &model.nsets {
        report.push_str(&format!("nset\t{}\t{}\n", s.name, s.ids.len()));
    }
    for s in &model.elsets {
        report.push_str(&format!("elset\t{}\t{}\n", s.name, s.ids.len()));
    }
    if (parsed.0, parsed.1) != TESTED {
        report.push_str(&format!(
            "warning\tgmsh {version} is not the tested release {}.{}\n",
            TESTED.0, TESTED.1
        ));
    }
    for line in lines_with("Warning", [&run.stderr, &run.stdout]) {
        report.push_str(&format!("warning\t{line}\n"));
    }
    Ok(report)
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let options = parse_args(args)?;
    let dir = run_dir()?;
    let result = mesh(&options, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(report) => {
            print!("{report}");
            Ok(ExitCode::SUCCESS)
        }
        Err(message) => {
            eprintln!("fh mesh: {message}");
            Ok(ExitCode::from(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(unit: &str) -> String {
        format!(
            "DATA;\n#1=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n#2=({unit});\nENDSEC;"
        )
    }

    #[test]
    fn step_units_are_read() {
        let mm = step("LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.)");
        let m = step("LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.)");
        let inch = step("CONVERSION_BASED_UNIT('INCH',#9)LENGTH_UNIT()NAMED_UNIT(#8)");
        assert_eq!(step_unit(&mm), Ok("mm"));
        assert_eq!(step_unit(&m), Ok("m"));
        assert_eq!(step_unit(&inch), Ok("inch"));
        assert!(step_unit("DATA;ENDSEC;")
            .unwrap_err()
            .starts_with("unit.step.missing"));
    }

    #[test]
    fn version_is_parsed() {
        assert_eq!(version_of("4.15.2\n"), Some((4, 15)));
        assert_eq!(version_of("4.15.2-git"), Some((4, 15)));
        assert_eq!(version_of("x"), None);
    }
}
