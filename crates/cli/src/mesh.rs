//! `fh mesh FILE.(geo|step) --units U [options]`: run the external gmsh program and write the
//! mesh as the nodes, the C3D10 elements (S6 shells with `--shell`) and the named sets of a
//! CalculiX input.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::rules;

const USAGE: &str = "usage: fh mesh FILE.(geo|step) --units SI-mm|SI-m [-o OUT.inp] [--size H] \
[--scale S] [--set NAME=VALUE]... [--face NAME=SELECTION]... [--nodes NAME=SELECTION]... \
[--volume NAME=box:X0,Y0,Z0,X1,Y1,Z1]... [--refine NAME[,NAME]...=H]... [--shell] \
[--timeout SECONDS] [--gmsh PATH]\n\
A selection is x:V, y:V or z:V (a plane), box:X0,Y0,Z0,X1,Y1,Z1, or point:X,Y,Z (--nodes only).\n\
--face makes a node set and a surface of the faces of the part in the selection. --nodes makes a \
node set. --volume makes an element set (STEP only). Each name is used once.\n\
--refine makes the elements at the --face sets NAME the size H. They grow to --size away from the sets.\n\
--shell meshes the surfaces of the file, not a solid: the elements are S6 shells, --face picks \
edges (the surface is the edge of the shells) and --volume picks surfaces.\n\
The units are required. The mesh is order 2, C3D10, or S6 with --shell.\n\
v1 refuses a .geo file with SystemCall, NonBlockingSystemCall, OnelabRun, Include, \
or Merge of a file that is not .msh.";

/// The size of an element grows by this much for each unit of distance from a `--refine` set.
/// 0.3 is a ratio of 1.3 between neighbour elements.
const GROWTH: f64 = 0.3;

/// The most points that a `--refine` field samples on a curve (a surface: this many in each
/// direction).
const MAX_SAMPLING: u32 = 1000;

/// The oldest gmsh release that `fh mesh` accepts.
const OLDEST: (u32, u32) = (4, 8);

/// The unit system of the input. There is no default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Units {
    /// Length mm, force N, stress MPa, mass tonne, time s.
    SiMm,
    /// Length m, force N, stress Pa, mass kg, time s.
    SiM,
}

impl Units {
    fn parse(text: &str) -> Option<Units> {
        match text {
            "SI-mm" => Some(Units::SiMm),
            "SI-m" => Some(Units::SiM),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Units::SiMm => "SI-mm",
            Units::SiM => "SI-m",
        }
    }

    /// The length unit as gmsh names it (`Geometry.OCCTargetUnit`).
    fn gmsh_length(self) -> &'static str {
        match self {
            Units::SiMm => "MM",
            Units::SiM => "M",
        }
    }
}

/// Where a set is: a plane, a box or a point.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Select {
    Plane { axis: usize, at: f64 },
    Box { min: [f64; 3], max: [f64; 3] },
    Point([f64; 3]),
}

fn numbers<const N: usize>(text: &str) -> Option<[f64; N]> {
    let values: Vec<f64> = text
        .split(',')
        .map(|t| t.trim().parse().ok().filter(|v: &f64| v.is_finite()))
        .collect::<Option<_>>()?;
    values.try_into().ok()
}

fn parse_select(text: &str) -> Result<Select, String> {
    let bad = || {
        format!("bad selection `{text}`. Use x:V, y:V, z:V, box:X0,Y0,Z0,X1,Y1,Z1 or point:X,Y,Z")
    };
    let (kind, rest) = text.split_once(':').ok_or_else(bad)?;
    match kind {
        "x" | "y" | "z" => {
            let [at] = numbers::<1>(rest).ok_or_else(bad)?;
            let axis = ["x", "y", "z"].iter().position(|a| *a == kind);
            Ok(Select::Plane {
                axis: axis.unwrap_or(0),
                at,
            })
        }
        "box" => {
            let b = numbers::<6>(rest).ok_or_else(bad)?;
            let (min, max) = ([b[0], b[1], b[2]], [b[3], b[4], b[5]]);
            if (0..3).any(|k| min[k] > max[k]) {
                return Err(format!(
                    "bad selection `{text}`: a minimum is above its maximum"
                ));
            }
            Ok(Select::Box { min, max })
        }
        "point" => Ok(Select::Point(numbers::<3>(rest).ok_or_else(bad)?)),
        _ => Err(bad()),
    }
}

/// A set name and where it is.
type Named = (String, Select);

struct Options {
    input: PathBuf,
    output: PathBuf,
    size: Option<f64>,
    scale: Option<f64>,
    numbers: Vec<(String, f64)>,
    units: Option<Units>,
    /// Faces of the part, from gmsh: a node set and a surface each.
    faces: Vec<Named>,
    /// Nodes, picked after the mesh: a node set each.
    nodes: Vec<Named>,
    /// Boxes of volume: an element set each.
    volumes: Vec<Named>,
    /// Element size at a `--face` set, from `--refine`. It grows to `size` away from the set.
    refine: Vec<(String, f64)>,
    /// Mesh the surfaces as shells. Faces are edges and volumes are surfaces.
    shell: bool,
    /// `None` means no limit: a large model can mesh for hours.
    timeout: Option<Duration>,
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

/// `NAME=SELECTION` of a set option.
fn named(flag: &str, text: Option<&String>) -> Result<Named, String> {
    let text = text.ok_or(format!("{flag} needs NAME=SELECTION"))?;
    let (name, select) = text
        .split_once('=')
        .ok_or(format!("{flag} needs NAME=SELECTION"))?;
    let name_ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !name_ok || name.eq_ignore_ascii_case("ALL") || name.eq_ignore_ascii_case("NALL") {
        return Err(format!(
            "{flag}: bad name `{name}`. Use letters, digits and _, and not ALL or NALL"
        ));
    }
    Ok((
        name.to_owned(),
        parse_select(select).map_err(|e| format!("{flag} {name}: {e}"))?,
    ))
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut input: Option<PathBuf> = None;
    let mut o = Options {
        input: PathBuf::new(),
        output: PathBuf::new(),
        size: None,
        scale: None,
        numbers: Vec::new(),
        units: None,
        faces: Vec::new(),
        nodes: Vec::new(),
        volumes: Vec::new(),
        refine: Vec::new(),
        shell: false,
        timeout: None,
        gmsh: None,
    };
    let mut output: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => output = Some(it.next().ok_or("-o needs a file")?.into()),
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
                let units = it.next().and_then(|t| Units::parse(t));
                o.units = Some(units.ok_or("--units needs SI-mm or SI-m")?);
            }
            "--face" => {
                let face = named("--face", it.next())?;
                if matches!(face.1, Select::Point(_)) {
                    return Err("--face needs x:V, y:V, z:V or box:X0,Y0,Z0,X1,Y1,Z1".into());
                }
                o.faces.push(face);
            }
            "--nodes" => o.nodes.push(named("--nodes", it.next())?),
            "--volume" => {
                let volume = named("--volume", it.next())?;
                if !matches!(volume.1, Select::Box { .. }) {
                    return Err("--volume needs box:X0,Y0,Z0,X1,Y1,Z1".into());
                }
                o.volumes.push(volume);
            }
            "--refine" => {
                let text = it.next().ok_or("--refine needs NAME=H")?;
                let (names, size) = text.split_once('=').ok_or("--refine needs NAME=H")?;
                let size = number("--refine", Some(&size.to_string()))?;
                for name in names.split(',') {
                    o.refine.push((name.to_owned(), size));
                }
            }
            "--shell" => o.shell = true,
            "--timeout" => {
                let seconds = number("--timeout", it.next())?;
                o.timeout = Some(
                    Duration::try_from_secs_f64(seconds)
                        .map_err(|_| "--timeout is too large".to_string())?,
                );
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
    o.output = output.unwrap_or_else(|| o.input.with_extension("inp"));
    let mut seen: Vec<String> = Vec::new();
    for (name, _) in o.faces.iter().chain(&o.nodes).chain(&o.volumes) {
        let upper = name.to_ascii_uppercase();
        if seen.contains(&upper) {
            return Err(format!(
                "the name {name} is used twice. Give each set its own name"
            ));
        }
        seen.push(upper);
    }
    let mut refined: Vec<String> = Vec::new();
    for (name, _) in &o.refine {
        if refined.contains(&name.to_ascii_uppercase()) {
            return Err(format!("--refine names {name} twice"));
        }
        refined.push(name.to_ascii_uppercase());
    }
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

/// Physical volume groups select the material boxes, or SOLID for the single body.
/// Each face option has its own physical surface group. It holds the faces of the part that
/// lie fully inside its plane or box, within a millionth of the diagonal of the part.
/// With `shell`, the body is a set of surfaces (group SHELL) and a face is a curve.
/// A face in `refine` also gets a size field: size `h` on the face, growing by `GROWTH` for each
/// unit of distance up to `far`. `refine` is empty when `far` is 0.
fn groups_script(
    faces: &[Named],
    volumes: &[Named],
    shell: bool,
    refine: &[(String, f64)],
    far: f64,
) -> String {
    let (body, side, default) = if shell {
        ("Surface", "Curve", "SHELL")
    } else {
        ("Volume", "Surface", "SOLID")
    };
    let mut s = String::new();
    if volumes.is_empty() {
        s.push_str(&format!(
            "fh_v[] = {body}{{:}};\nPhysical {body}(\"{default}\") = fh_v[];\n"
        ));
    } else {
        // Fragment the touching bodies so both layers share every interface node.
        s.push_str(&format!("BooleanFragments{{ {body}{{:}}; Delete; }}{{}}\n"));
        for (name, select) in volumes {
            if let Select::Box { min, max } = select {
                let lo = min.iter().map(|x| format!("{x:?}-1e-7"));
                let hi = max.iter().map(|x| format!("{x:?}+1e-7"));
                let coords = lo.chain(hi).collect::<Vec<_>>().join(", ");
                s.push_str(&format!(
                    "fh_m[] = {body} In BoundingBox{{{coords}}};\nPhysical {body}(\"{name}\") = fh_m[];\n"
                ));
            }
        }
        s.push_str(&format!("fh_v[] = {body}{{:}};\n"));
    }
    if faces.is_empty() {
        return s;
    }
    s.push_str(&format!(
        "fh_b[] = BoundingBox {body}{{fh_v[]}};\n\
fh_e = 1e-6 * Sqrt((fh_b[3] - fh_b[0])^2 + (fh_b[4] - fh_b[1])^2 + (fh_b[5] - fh_b[2])^2);\n"
    ));
    if !refine.is_empty() {
        s.push_str("fh_k = 1;\nfh_f[] = {};\n");
    }
    for (name, select) in faces {
        let (lo, hi) = match select {
            Select::Plane { axis, at } => {
                let mut lo = ["-1e30".to_string(), "-1e30".into(), "-1e30".into()];
                let mut hi = ["1e30".to_string(), "1e30".into(), "1e30".into()];
                lo[*axis] = format!("{at:?} - fh_e");
                hi[*axis] = format!("{at:?} + fh_e");
                (lo, hi)
            }
            Select::Box { min, max } => (
                min.map(|v| format!("{v:?} - fh_e")),
                max.map(|v| format!("{v:?} + fh_e")),
            ),
            Select::Point(_) => continue,
        };
        s.push_str(&format!(
            "fh_s[] = {side} In BoundingBox{{{}, {}, {}, {}, {}, {}}};\nPhysical {side}(\"{name}\") = fh_s[];\n",
            lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
        ));
        let Some((_, h)) = refine.iter().find(|r| r.0.eq_ignore_ascii_case(name)) else {
            continue;
        };
        // The Distance field samples each entity. Sample twice for each element of size h.
        let list = if shell { "CurvesList" } else { "SurfacesList" };
        s.push_str(&format!(
            "For fh_i In {{0 : #fh_s[] - 1}}\n\
fh_x[] = BoundingBox {side}{{fh_s[fh_i]}};\n\
Field[fh_k] = Distance;\n\
Field[fh_k].{list} = {{fh_s[fh_i]}};\n\
Field[fh_k].Sampling = Min({MAX_SAMPLING}, Ceil(2 * Max(fh_x[3] - fh_x[0], Max(fh_x[4] - fh_x[1], fh_x[5] - fh_x[2])) / {h}));\n\
Field[fh_k + 1] = Threshold;\n\
Field[fh_k + 1].InField = fh_k;\n\
Field[fh_k + 1].SizeMin = {h};\n\
Field[fh_k + 1].SizeMax = {far};\n\
Field[fh_k + 1].DistMin = 0;\n\
Field[fh_k + 1].DistMax = {};\n\
fh_f[] += {{fh_k + 1}};\n\
fh_k += 2;\n\
EndFor\n",
            (far - h) / GROWTH
        ));
    }
    if !refine.is_empty() {
        // The sizes come from the field only, not from the points or the boundary.
        s.push_str(
            "Field[fh_k] = Min;\nField[fh_k].FieldsList = {fh_f[]};\nBackground Field = fh_k;\n\
Mesh.MeshSizeExtendFromBoundary = 0;\nMesh.MeshSizeFromPoints = 0;\nMesh.MeshSizeFromCurvature = 0;\n",
        );
    }
    s
}

/// Refuse a STEP file whose length unit is missing, mixed, or not the declared unit.
fn check_step(text: &str, units: Units) -> Result<(), String> {
    let unit = fh_io::step_length_unit(text).map_err(|e| format!("{}: {e}", e.code()))?;
    if unit.gmsh_name() == Some(units.gmsh_length()) {
        Ok(())
    } else {
        Err(format!(
            "unit.step.mismatch: the STEP file uses {unit}, but the units are {}",
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

/// Run a command in `dir`, with a time limit when `limit` is set. Kill it on expiry.
/// The output goes to files, so a full pipe cannot block the child.
fn run_limited(
    gmsh: &Path,
    args: &[String],
    dir: &Path,
    limit: Option<Duration>,
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
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if let Some(limit) = limit.filter(|l| start.elapsed() >= *l) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "gmsh timed out after {} seconds and was stopped",
                limit.as_secs_f64()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
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

/// Write one id list with at most 16 ids on a line.
fn id_lines(out: &mut String, ids: &[u32]) {
    for chunk in ids.chunks(16) {
        let line: Vec<String> = chunk.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "{}", line.join(", "));
    }
}

/// The ids of the nodes in a selection, within `tol`.
fn pick_nodes(model: &fh_inp::Model, select: &Select, tol: f64) -> Vec<u32> {
    let inside = |xyz: &[f64; 3]| match select {
        Select::Plane { axis, at } => (xyz[*axis] - at).abs() <= tol,
        Select::Box { min, max } => {
            (0..3).all(|k| xyz[k] >= min[k] - tol && xyz[k] <= max[k] + tol)
        }
        Select::Point(p) => (0..3).all(|k| (xyz[k] - p[k]).abs() <= tol),
    };
    model
        .nodes
        .iter()
        .filter(|n| inside(&n.xyz))
        .map(|n| n.id)
        .collect()
}

/// The node sets of the `--nodes` options, as ids. Refuse a name that the mesh uses, and a
/// selection with no node.
fn pick_sets(model: &fh_inp::Model, picks: &[Named]) -> Result<Vec<(String, Vec<u32>)>, String> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for n in &model.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.xyz[k]);
            hi[k] = hi[k].max(n.xyz[k]);
        }
    }
    let tol = 1e-6 * (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
    let mut sets = Vec::new();
    for (name, select) in picks {
        if model
            .nsets
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(name))
        {
            return Err(format!(
                "mesh.name.duplicate: the mesh has a node set {name} already. Give --nodes {name} another name"
            ));
        }
        let ids = pick_nodes(model, select, tol);
        if ids.is_empty() {
            return Err(format!(
                "mesh.set.empty: --nodes {name} ({select:?}, tolerance {tol:e}) holds no node"
            ));
        }
        sets.push((name.clone(), ids));
    }
    Ok(sets)
}

/// The text of the input. The nodes are in the set `NALL` and the elements in the set `ALL`.
/// Then come the sets of the mesh, the surfaces, and the `picked` node sets. The sets named
/// `hide` are not written: the default body group holds the same elements as ALL.
fn write_inp(
    model: &fh_inp::Model,
    label: &str,
    units: Units,
    picked: &[(String, Vec<u32>)],
    hide: Option<&str>,
) -> String {
    // The solver run reads the units line and writes the units into the results.
    let mut out = format!(
        "** Made by fh mesh from {label}. Include it in the job file.\n** units: {}\n*NODE, NSET=NALL\n",
        units.name()
    );
    // CalculiX reads 20 characters of each number: 13 digits fit with any exponent.
    let fit = |x: f64| match format!("{x:?}") {
        s if s.len() <= 20 => s,
        _ => format!("{x:.12e}"),
    };
    for n in &model.nodes {
        let [x, y, z] = n.xyz.map(fit);
        let _ = writeln!(out, "{}, {x}, {y}, {z}", n.id);
    }
    let element_type = model
        .elements
        .first()
        .map_or("", |e| e.element_type.as_str());
    let _ = writeln!(out, "*ELEMENT, TYPE={element_type}, ELSET=ALL");
    for e in &model.elements {
        let nodes: Vec<String> = e.nodes.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "{}, {}", e.id, nodes.join(", "));
    }
    let shown = |name: &str| Some(name) != hide;
    for s in model.nsets.iter().filter(|s| shown(&s.name)) {
        let _ = writeln!(out, "*NSET, NSET={}", s.name);
        id_lines(&mut out, &s.ids);
    }
    for s in model.elsets.iter().filter(|s| shown(&s.name)) {
        let _ = writeln!(out, "*ELSET, ELSET={}", s.name);
        id_lines(&mut out, &s.ids);
    }
    for s in &model.element_surfaces {
        let _ = writeln!(out, "*SURFACE, TYPE=ELEMENT, NAME={}", s.name);
        for (element, face) in &s.faces {
            let _ = writeln!(out, "{element}, S{face}");
        }
    }
    for (name, ids) in picked {
        let _ = writeln!(out, "*NSET, NSET={name}");
        id_lines(&mut out, ids);
    }
    out
}

/// What a run makes, or a failure text.
fn mesh(o: &Options, dir: &Path) -> Result<String, String> {
    let units = o
        .units
        .ok_or("unit.missing: the run has no units. Give --units SI-mm or --units SI-m")?;
    let input = std::path::absolute(&o.input).map_err(|e| e.to_string())?;
    let output = std::path::absolute(&o.output).map_err(|e| e.to_string())?;
    if input == output {
        return Err("the output file is the input file".into());
    }
    let extension = input
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    for (name, h) in &o.refine {
        if !o.faces.iter().any(|f| f.0.eq_ignore_ascii_case(name)) {
            return Err(format!("--refine {name}: there is no --face {name}"));
        }
        match o.size {
            Some(far) if h < &far => {}
            Some(far) => return Err(format!("--refine {name}: {h} must be below --size {far}")),
            None => {
                return Err(format!(
                    "--refine {name}: it needs --size, the size far from the set"
                ))
            }
        }
    }
    let bytes = std::fs::read(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let script = match extension.as_deref() {
        Some("geo") => {
            if !o.faces.is_empty() || !o.volumes.is_empty() {
                return Err(
                    "--face and --volume need a .step file. A .geo file names its own groups"
                        .into(),
                );
            }
            check_geo(&text)?;
            input.clone()
        }
        Some("step" | "stp") => {
            check_step(&text, units)?;
            // Pin the target unit in every run. The gmsh default is a process state.
            let wrapper = dir.join("input.geo");
            let body =
                format!(
                "SetFactory(\"OpenCASCADE\");\nGeometry.OCCTargetUnit = \"{}\";\nMerge \"{}\";\n{}",
                units.gmsh_length(),
                quote_ok(&input)?,
                groups_script(&o.faces, &o.volumes, o.shell, &o.refine, o.size.unwrap_or(0.0))
            );
            std::fs::write(&wrapper, body).map_err(|e| e.to_string())?;
            wrapper
        }
        _ => return Err("the input must be a .geo or a .step file".into()),
    };
    let gmsh = find_gmsh(o.gmsh.as_deref())?;

    let probe = run_limited(
        &gmsh,
        &["-version".into()],
        dir,
        Some(Duration::from_secs(30)),
    )?;
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
    let msh_file = dir.join("mesh.msh");
    // Order 2 gives C3D10 (S6 for a surface), the element that `fh mesh` writes.
    let mut args: Vec<String> = [
        if o.shell { "-2" } else { "-3" },
        "-nopopup",
        "-format",
        "msh41",
        "-v",
        "2",
        "-order",
        "2",
    ]
    .map(String::from)
    .into();
    for (flag, value) in [("-clmax", o.size), ("-clscale", o.scale)] {
        if let Some(v) = value {
            args.extend([flag.into(), v.to_string()]);
        }
    }
    for (name, value) in &o.numbers {
        args.extend(["-setnumber".into(), name.clone(), value.to_string()]);
    }
    args.extend(["-o".into(), msh_file.to_string_lossy().into_owned()]);
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
    let msh = std::fs::read_to_string(&msh_file).map_err(|_| "gmsh wrote no mesh file")?;
    let read = if o.shell {
        fh_io::read_msh_shell
    } else {
        fh_io::read_msh_mesh
    };
    let mesh = read(&msh)
        .map_err(|e| format!("the mesh is not readable: {}", e.with_file("gmsh output")))?;
    let model = &mesh.model;
    if o.shell {
        rules::no_solid(&mesh)?;
    }
    if model.elements.is_empty() {
        return Err(if o.shell {
            "gmsh wrote a mesh with no surface elements".into()
        } else {
            "gmsh wrote a mesh with no volume elements. For a STEP file of surfaces, use --shell"
                .into()
        });
    }
    let stats = fh_io::mesh_stats(model)?;
    if stats.inverted > 0 {
        return Err(format!(
            "mesh.jacobian.negative: {} elements have a corner with a Jacobian of 0 or less, \
for example {:?}",
            stats.inverted, stats.inverted_ids
        ));
    }
    rules::volumes_are_named(&mesh, o.shell)?;
    let volume_names: Vec<&str> = o.volumes.iter().map(|v| v.0.as_str()).collect();
    rules::volume_coverage(&mesh, &volume_names)?;
    for (name, _) in &o.faces {
        rules::face_surface(&mesh, name, o.shell)?;
    }

    let label = input
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    // The default body group of a STEP file holds the same elements as ALL.
    let hide = (extension.as_deref() != Some("geo") && o.volumes.is_empty())
        .then_some(if o.shell { "SHELL" } else { "SOLID" });
    let picked = pick_sets(model, &o.nodes)?;
    let inp = write_inp(model, &label, units, &picked, hide);
    std::fs::write(&output, inp).map_err(|e| format!("{}: {e}", output.display()))?;

    let mut report = format!(
        "ok\t{}\ngmsh\t{version}\nunits\t{}\nnodes\t{}\nelements\t{}\n\
min_scaled_jacobian\t{:.4}\nnset\tNALL\t{}\nelset\tALL\t{}\n",
        output.display(),
        units.name(),
        model.nodes.len(),
        model.elements.len(),
        stats.min_scaled_jacobian,
        model.nodes.len(),
        model.elements.len(),
    );
    for s in model.nsets.iter().filter(|s| Some(s.name.as_str()) != hide) {
        report.push_str(&format!("nset\t{}\t{}\n", s.name, s.ids.len()));
    }
    for s in model
        .elsets
        .iter()
        .filter(|s| Some(s.name.as_str()) != hide)
    {
        report.push_str(&format!("elset\t{}\t{}\n", s.name, s.ids.len()));
    }
    for s in &model.element_surfaces {
        report.push_str(&format!("surface\t{}\t{}\n", s.name, s.faces.len()));
    }
    for (name, ids) in &picked {
        report.push_str(&format!("nset\t{name}\t{}\n", ids.len()));
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

    fn args(a: &[&str]) -> Result<Options, String> {
        parse_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn step_units_are_checked_against_the_declared_units() {
        let mm = step("LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.)");
        let inch = step("CONVERSION_BASED_UNIT('INCH',#9)LENGTH_UNIT()NAMED_UNIT(#8)");
        assert_eq!(check_step(&mm, Units::SiMm), Ok(()));
        assert!(check_step(&mm, Units::SiM)
            .unwrap_err()
            .starts_with("unit.step.mismatch"));
        assert!(check_step(&inch, Units::SiMm)
            .unwrap_err()
            .starts_with("unit.step.mismatch"));
        assert!(check_step("DATA;ENDSEC;", Units::SiMm)
            .unwrap_err()
            .starts_with("unit.step.missing"));
    }

    #[test]
    fn gmsh_has_a_time_limit_only_with_the_timeout_option() {
        assert_eq!(args(&["a.geo"]).unwrap().timeout, None);
        assert_eq!(
            args(&["a.geo", "--timeout", "2"]).unwrap().timeout,
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn version_is_parsed() {
        assert_eq!(version_of("4.15.2\n"), Some((4, 15)));
        assert_eq!(version_of("4.15.2-git"), Some((4, 15)));
        assert_eq!(version_of("x"), None);
    }

    #[test]
    fn selections_are_parsed() {
        assert_eq!(
            parse_select("x:20"),
            Ok(Select::Plane { axis: 0, at: 20.0 })
        );
        assert_eq!(
            parse_select("z:-1.5e-3"),
            Ok(Select::Plane {
                axis: 2,
                at: -0.0015
            })
        );
        assert_eq!(
            parse_select("box:0,1,2,3,4,5"),
            Ok(Select::Box {
                min: [0.0, 1.0, 2.0],
                max: [3.0, 4.0, 5.0]
            })
        );
        assert_eq!(
            parse_select("point: 1, 2, 3"),
            Ok(Select::Point([1.0, 2.0, 3.0]))
        );
        for bad in [
            "",
            "x",
            "x:a",
            "x:1,2",
            "w:1",
            "box:1,2,3",
            "box:5,0,0,1,1,1",
            "point:1,2",
            "x:inf",
        ] {
            assert!(parse_select(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn set_options_are_checked() {
        let o = args(&[
            "a.step",
            "--face",
            "TOP=z:1",
            "--face",
            "P=box:0,0,0,1,1,1",
            "--nodes",
            "PIN=point:0,0,0",
            "--volume",
            "UPPER=box:0,0,1,2,2,2",
        ])
        .unwrap();
        assert_eq!((o.faces.len(), o.nodes.len(), o.volumes.len()), (2, 1, 1));
        assert_eq!(o.output, PathBuf::from("a.inp"));
        for bad in [
            &["a.step", "--face", "TOP"][..],
            &["a.step", "--face", "TOP=point:0,0,0"],
            &["a.step", "--volume", "V=z:1"],
            &["a.step", "--face", "ALL=z:1"],
            &["a.step", "--nodes", "nall=z:1"],
            &["a.step", "--nodes", "A B=z:1"],
            &["a.step", "--face", "TOP=z:1", "--nodes", "top=z:0"],
            &["a.step", "--face"],
        ] {
            assert!(args(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_gmsh_script_names_the_groups() {
        let faces = vec![
            ("TOP".to_owned(), Select::Plane { axis: 2, at: 5.0 }),
            (
                "BOX".to_owned(),
                Select::Box {
                    min: [0.0; 3],
                    max: [1.0; 3],
                },
            ),
        ];
        let one = groups_script(&faces, &[], false, &[], 0.0);
        assert!(one.contains("Physical Volume(\"SOLID\")"), "{one}");
        assert!(one.contains("Physical Surface(\"TOP\")"), "{one}");
        assert!(
            one.contains("-1e30, -1e30, 5.0 - fh_e, 1e30, 1e30, 5.0 + fh_e"),
            "{one}"
        );
        assert!(one.contains("Physical Surface(\"BOX\")"), "{one}");
        let layers = vec![
            (
                "LOW".to_owned(),
                Select::Box {
                    min: [0.0; 3],
                    max: [2.0, 2.0, 1.0],
                },
            ),
            (
                "UP".to_owned(),
                Select::Box {
                    min: [0.0, 0.0, 1.0],
                    max: [2.0; 3],
                },
            ),
        ];
        let two = groups_script(&[], &layers, false, &[], 0.0);
        assert!(two.contains("BooleanFragments"), "{two}");
        assert!(two.contains("Physical Volume(\"LOW\")"), "{two}");
        assert!(two.contains("Physical Volume(\"UP\")"), "{two}");
        assert!(!two.contains("SOLID"), "{two}");
        // A shell has surfaces for volumes and curves for faces.
        let shell = groups_script(&faces, &[], true, &[], 0.0);
        assert!(shell.contains("Physical Surface(\"SHELL\")"), "{shell}");
        assert!(shell.contains("Physical Curve(\"TOP\")"), "{shell}");
        assert!(shell.contains("Curve In BoundingBox"), "{shell}");
        let parts = groups_script(&[], &layers, true, &[], 0.0);
        assert!(parts.contains("BooleanFragments{ Surface{:}"), "{parts}");
        assert!(parts.contains("Surface In BoundingBox"), "{parts}");
        assert!(parts.contains("Physical Surface(\"LOW\")"), "{parts}");
    }

    #[test]
    fn refine_gives_a_size_to_each_named_set() {
        let o = args(&["a.step", "--refine", "A,b=0.5", "--refine", "C=2"]).unwrap();
        let sizes: Vec<(&str, f64)> = o.refine.iter().map(|(n, h)| (n.as_str(), *h)).collect();
        assert_eq!(sizes, [("A", 0.5), ("b", 0.5), ("C", 2.0)]);
        for bad in [
            &["a.step", "--refine"][..],
            &["a.step", "--refine", "A"],
            &["a.step", "--refine", "A=0"],
            &["a.step", "--refine", "A=x"],
            &["a.step", "--refine", "A=1", "--refine", "a=2"],
            &["a.step", "--refine", "A,a=1"],
        ] {
            assert!(args(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_shell_option_is_a_flag() {
        assert!(!args(&["a.step"]).unwrap().shell);
        assert!(args(&["a.step", "--shell"]).unwrap().shell);
    }

    fn block() -> fh_io::MshMesh {
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/msh/block-tet10.msh"),
        )
        .unwrap();
        fh_io::read_msh_mesh(&text).unwrap()
    }

    #[test]
    fn the_input_holds_the_mesh_the_units_and_the_sets() {
        let mesh = block();
        let model = &mesh.model;
        let picks = [
            ("ORIGIN".to_owned(), Select::Point([0.0, 0.0, 0.0])),
            ("MID".to_owned(), Select::Plane { axis: 0, at: 2.0 }),
        ];
        let picked = pick_sets(model, &picks).unwrap();
        assert_eq!(picked[0].1.len(), 1);
        assert!(picked[1].1.len() > 4, "{picked:?}");
        let text = write_inp(model, "block.step", Units::SiMm, &picked, None);
        assert!(
            text.contains("** units: SI-mm\n*NODE, NSET=NALL\n"),
            "{text}"
        );
        assert!(text.contains("*ELEMENT, TYPE=C3D10, ELSET=ALL\n"), "{text}");
        for set in [
            "*NSET, NSET=FIX",
            "*NSET, NSET=ORIGIN",
            "*ELSET, ELSET=BLOCK",
            "*SURFACE, TYPE=ELEMENT, NAME=TIP",
        ] {
            assert!(text.contains(set), "{set}");
        }
        // The reader of the preview sees the same mesh.
        let back = fh_inp::parse(&text);
        assert_eq!(back.nodes.len(), model.nodes.len());
        assert_eq!(back.elements.len(), model.elements.len());
        assert_eq!(back.nset("ORIGIN").unwrap().ids, picked[0].1);
        assert_eq!(
            back.element_surface("TIP").unwrap().faces,
            model.element_surface("TIP").unwrap().faces
        );
        // A line holds at most 16 ids, the limit of CalculiX.
        assert!(text.lines().all(|l| l.split(',').count() <= 16), "{text}");
        let hidden = write_inp(model, "block.step", Units::SiMm, &[], Some("BLOCK"));
        assert!(!hidden.contains("*ELSET, ELSET=BLOCK"));
    }

    #[test]
    fn a_coordinate_has_at_most_20_characters() {
        // CalculiX reads 20 characters of each number. gmsh gives round-off values near 0.
        let mut model = block().model;
        let xyz = [
            -4.874999160318342e-15,
            1.2345678901234567e-4,
            19.90369453344394,
        ];
        model.nodes[0].xyz = xyz;
        let text = write_inp(&model, "block.step", Units::SiMm, &[], None);
        let line = text.lines().nth(3).unwrap();
        assert!(line.split(", ").all(|w| w.len() <= 20), "{line}");
        let back = fh_inp::parse(&text).nodes[0].xyz;
        for (b, x) in back.iter().zip(xyz) {
            assert!((b - x).abs() <= 1e-12 * x.abs(), "{b} {x}");
        }
    }

    #[test]
    fn a_node_set_with_no_node_or_a_used_name_is_refused() {
        let mesh = block();
        let far = [("FAR".to_owned(), Select::Point([9.0, 9.0, 9.0]))];
        assert!(pick_sets(&mesh.model, &far)
            .unwrap_err()
            .starts_with("mesh.set.empty"));
        let used = [("fix".to_owned(), Select::Plane { axis: 0, at: 0.0 })];
        assert!(pick_sets(&mesh.model, &used)
            .unwrap_err()
            .starts_with("mesh.name.duplicate"));
    }
}
