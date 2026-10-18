//! `fh view MODEL.inp|RUN_DIR|results.pvd`: open the viewer on a mesh or on the results of a run.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: fh view MODEL.inp|RUN_DIR|results.pvd";

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let [file] = args else {
        return Err(USAGE.to_string());
    };
    if file.starts_with('-') {
        return Err(format!("unknown option: {file}\n{USAGE}"));
    }
    fh_viewer::view_file(&PathBuf::from(file))?;
    Ok(ExitCode::SUCCESS)
}
