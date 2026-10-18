//! `fh info FILE [--case DIR]`: unit and mesh checks for a STEP file or a `.msh` file.
//! Decisions D-18 and D-19, see DESIGN.md.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fh_validation::{Case, Units};

/// Print a refusal with its stable code. The exit code is 1.
fn refuse(code: &str, message: String) -> ExitCode {
    eprintln!("fh info: refused: {code}: {message}");
    ExitCode::from(1)
}

fn step(file: &Path, text: &str, case: Option<&Case>) -> ExitCode {
    let unit = match fh_io::step_length_unit(text) {
        Ok(unit) => unit,
        Err(e) => return refuse(e.code(), format!("{}: {e}", file.display())),
    };
    println!("file: {}\nkind: step\nstep_unit: {unit}", file.display());
    let Some(case) = case else {
        return ExitCode::SUCCESS;
    };
    println!("case_units: {}", case.units.name());
    if unit.gmsh_name() == Some(case.units.gmsh_length()) {
        println!(
            "units_match: yes\ngmsh_target_unit: {}",
            case.units.gmsh_length()
        );
        return ExitCode::SUCCESS;
    }
    refuse(
        "unit.step.mismatch",
        format!(
            "{} is in {unit}, but case {} declares {}. Export the STEP file in {} or change the case.",
            file.display(),
            case.id,
            case.units.name(),
            case.units.gmsh_length().to_lowercase()
        ),
    )
}

fn msh(file: &Path, text: &str, units: Option<Units>) -> Result<ExitCode, String> {
    let model =
        fh_io::read_msh(text).map_err(|e| e.with_file(file.display().to_string()).to_string())?;
    let s = fh_io::mesh_stats(&model)?;
    if s.types.is_empty() {
        return Ok(refuse(
            "mesh.empty",
            format!(
                "{} has no solid elements of a supported type",
                file.display()
            ),
        ));
    }
    let types: Vec<String> = s.types.iter().map(|(t, n)| format!("{t}={n}")).collect();
    let length = units.map_or("", |u| if u == Units::SiMm { " mm" } else { " m" });
    println!(
        "file: {}\nkind: msh\nnodes: {}\nelements: {}",
        file.display(),
        s.nodes,
        types.join(" ")
    );
    println!(
        "bbox_min: {:?}{length}\nbbox_max: {:?}{length}",
        s.bbox_min, s.bbox_max
    );
    println!(
        "volume: {:.6e}\nmin_scaled_jacobian: {:.4}",
        s.volume, s.min_scaled_jacobian
    );
    if let Err(message) = crate::rules::hex_needs_extrusion(&model) {
        let (code, text) = message
            .split_once(": ")
            .unwrap_or(("mesh.hex.not_extruded", &message));
        return Ok(refuse(code, text.to_string()));
    }
    if s.inverted > 0 {
        return Ok(refuse(
            "mesh.jacobian.negative",
            format!(
                "{} elements have a corner with a Jacobian of 0 or less, for example {:?}",
                s.inverted, s.inverted_ids
            ),
        ));
    }
    Ok(ExitCode::SUCCESS)
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<PathBuf> = None;
    let mut case_dir: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--case" => case_dir = Some(it.next().ok_or("--case needs a directory")?.into()),
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            s if file.is_none() => file = Some(s.into()),
            s => return Err(format!("unexpected argument: {s}")),
        }
    }
    let file = file.ok_or("usage: fh info FILE.step|FILE.msh [--case DIR]")?;
    let case = case_dir
        .map(|dir| Case::load_before_mesh(&dir).map_err(|e| e.to_string()))
        .transpose()?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    match file.extension().and_then(|x| x.to_str()) {
        Some("step" | "stp") => Ok(step(&file, &text, case.as_ref())),
        Some("msh") => msh(&file, &text, case.as_ref().map(|c| c.units)),
        _ => Err(format!(
            "{}: expected a .step, .stp or .msh file",
            file.display()
        )),
    }
}
