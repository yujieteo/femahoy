//! `fh view MODEL.inp`: open the mesh-only viewer on a model.

use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let [file] = args else {
        return Err("usage: fh view MODEL.inp".to_string());
    };
    if file.starts_with('-') {
        return Err(format!("unknown option: {file}\nusage: fh view MODEL.inp"));
    }
    fh_viewer::view_file(&PathBuf::from(file))?;
    Ok(ExitCode::SUCCESS)
}
