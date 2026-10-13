//! `fh model FILE.msh --case DIR [-o OUT.inp] [--mesh-units SI-mm|SI-m]`: write a solver input
//! from a gmsh mesh and the `[model]` table of the case. Decisions D-12 and D-19, see DESIGN.md.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fh_validation::{Case, ModelSpec, Units};

/// The file that `fh mesh` writes beside a mesh to record its units.
pub fn units_file(msh: &Path) -> PathBuf {
    let mut name = msh.as_os_str().to_owned();
    name.push(".units");
    name.into()
}

/// Decide the units of the mesh, and refuse when they differ from the case. D-18: no default.
fn mesh_units(msh: &Path, option: Option<Units>, case: Units) -> Result<(), String> {
    let recorded = match std::fs::read_to_string(units_file(msh)) {
        Ok(text) => Some(
            Units::parse(text.trim())
                .ok_or_else(|| format!("unit.mesh.unreadable: {}", units_file(msh).display()))?,
        ),
        Err(_) => None,
    };
    let units = match (recorded, option) {
        (Some(a), Some(b)) if a != b => {
            return Err(format!(
                "unit.mesh.conflict: fh mesh recorded {}, but --mesh-units says {}",
                a.name(),
                b.name()
            ))
        }
        (Some(u), _) | (None, Some(u)) => u,
        (None, None) => {
            return Err(format!(
                "unit.mesh.missing: {} has no unit record. Make it with fh mesh, or give --mesh-units SI-mm|SI-m",
                msh.display()
            ))
        }
    };
    if units == case {
        return Ok(());
    }
    Err(format!(
        "unit.mesh.mismatch: the mesh is in {}, but the case declares {}. Mesh again with the case units",
        units.name(),
        case.name()
    ))
}

/// Write one id list with at most 16 ids on a line.
fn id_lines(out: &mut String, ids: &[u32]) {
    for chunk in ids.chunks(16) {
        let line: Vec<String> = chunk.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "{}", line.join(", "));
    }
}

/// The text of the solver input. `Err` holds a refusal code and text.
fn write_inp(model: &fh_inp::Model, spec: &ModelSpec, label: &str) -> Result<String, String> {
    let mut types: Vec<&str> = model
        .elements
        .iter()
        .map(|e| e.element_type.as_str())
        .collect();
    types.dedup();
    types.sort_unstable();
    types.dedup();
    let [element_type] = types[..] else {
        return Err(if types.is_empty() {
            "mesh.empty: the mesh has no solid element of a supported type (C3D10, C3D8I)".into()
        } else {
            format!("mesh.mixed: the mesh mixes element types {types:?}. Mesh one type")
        });
    };
    crate::rules::hex_needs_extrusion(model)?;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for n in &model.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.xyz[k]);
            hi[k] = hi[k].max(n.xyz[k]);
        }
    }
    let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
    let mut out =
        format!("** Made by fh model from {label}. Do not edit; change the case.\n*NODE\n");
    for n in &model.nodes {
        let _ = writeln!(
            out,
            "{}, {:?}, {:?}, {:?}",
            n.id, n.xyz[0], n.xyz[1], n.xyz[2]
        );
    }
    let _ = writeln!(out, "*ELEMENT, TYPE={element_type}");
    for e in &model.elements {
        let nodes: Vec<String> = e.nodes.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "{}, {}", e.id, nodes.join(", "));
    }
    out.push_str("*ELSET, ELSET=ALL\n");
    id_lines(
        &mut out,
        &model.elements.iter().map(|e| e.id).collect::<Vec<_>>(),
    );
    for s in &spec.sets {
        let tol = s.tol.unwrap_or(1e-6 * extent);
        let ids: Vec<u32> = model
            .nodes
            .iter()
            .filter(|n| (n.xyz[s.axis] - s.at).abs() <= tol)
            .map(|n| n.id)
            .collect();
        if ids.is_empty() {
            return Err(format!(
                "model.set.empty: set {} (plane {} = {}, tolerance {tol:e}) holds no node",
                s.name,
                ["x", "y", "z"][s.axis],
                s.at
            ));
        }
        let _ = writeln!(out, "*NSET, NSET={}", s.name);
        id_lines(&mut out, &ids);
    }
    let m = &spec.material;
    let _ = write!(
        out,
        "*MATERIAL, NAME={0}\n*ELASTIC\n{1:?}, {2:?}\n*SOLID SECTION, ELSET=ALL, MATERIAL={0}\n*STEP, NAME={3}\n*STATIC\n1.0, 1.0\n*BOUNDARY\n",
        m.name, m.youngs_modulus, m.poissons_ratio, spec.step
    );
    for b in &spec.boundaries {
        let _ = writeln!(
            out,
            "{}, {}, {}, {:?}",
            b.set, b.first_dof, b.last_dof, b.value
        );
    }
    if !spec.cloads.is_empty() {
        out.push_str("*CLOAD\n");
        for c in &spec.cloads {
            let _ = writeln!(out, "{}, {}, {:?}", c.set, c.dof, c.per_node);
        }
    }
    out.push_str("*END STEP\n");
    Ok(out)
}

fn refuse(message: &str) -> ExitCode {
    eprintln!("fh model: refused: {message}");
    ExitCode::from(1)
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    const USAGE: &str =
        "usage: fh model FILE.msh --case DIR [-o OUT.inp] [--mesh-units SI-mm|SI-m]";
    let (mut file, mut case_dir, mut out, mut units): (Option<PathBuf>, Option<PathBuf>, _, _) =
        (None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| it.next().ok_or(format!("{a} needs {what}"));
        match a.as_str() {
            "--case" => case_dir = Some(value("a directory")?.into()),
            "-o" => out = Some(PathBuf::from(value("a file")?)),
            "--mesh-units" => {
                units = Some(
                    Units::parse(value("SI-mm or SI-m")?)
                        .ok_or("--mesh-units needs SI-mm or SI-m")?,
                )
            }
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            s if file.is_none() => file = Some(s.into()),
            s => return Err(format!("unexpected argument: {s}")),
        }
    }
    let (Some(file), Some(case_dir)) = (file, case_dir) else {
        return Err(USAGE.into());
    };
    let case = Case::load(&case_dir).map_err(|e| e.to_string())?;
    let Some(spec) = &case.model else {
        return Ok(refuse(&format!(
            "model.missing: {} has no [model] table",
            case_dir.join("case.toml").display()
        )));
    };
    if let Err(message) = mesh_units(&file, units, case.units) {
        return Ok(refuse(&message));
    }
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mesh = fh_io::read_msh(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let label = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let inp = match write_inp(&mesh, spec, &label) {
        Ok(inp) => inp,
        Err(message) => return Ok(refuse(&message)),
    };
    // The solver reads this text. Parse it here so that a bad input fails now.
    if let Err(e) = fh_inp::parse(&inp) {
        return Ok(refuse(&format!("model.inp.invalid: {e}")));
    }
    let out = out.unwrap_or_else(|| file.with_extension("inp"));
    std::fs::write(&out, inp).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "ok\t{}\nunits\t{}\nnodes\t{}\nelements\t{}",
        out.display(),
        case.units.name(),
        mesh.nodes.len(),
        mesh.elements.len()
    );
    Ok(ExitCode::SUCCESS)
}
