//! The text of an input: its units line, its `*INCLUDE` files and the global inputs of its
//! `*SUBMODEL` keywords.

use std::path::{Component, Path, PathBuf};

/// The units of an input from its line `** units: SI-mm`, or an empty text.
pub fn units(text: &str) -> String {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("** units:"))
        .map_or_else(String::new, |u| u.trim().to_owned())
}

/// The file of a line `*INCLUDE, INPUT=FILE`, as the line writes it.
fn include_of(line: &str) -> Option<&str> {
    input_of(line, "INCLUDE")
}

/// The value of the parameter `INPUT` of a line of the keyword `keyword`, as the line writes it.
fn input_of<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.trim().strip_prefix('*')?;
    let mut parts = rest.split(',');
    if !parts.next()?.trim().eq_ignore_ascii_case(keyword) {
        return None;
    }
    parts.find_map(|p| {
        let (name, value) = p.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("INPUT")
            .then(|| value.trim())
    })
}

/// The files that the text includes with `*INCLUDE, INPUT=FILE`, as the text writes them.
pub fn includes(text: &str) -> Vec<&str> {
    text.lines().filter_map(include_of).collect()
}

/// The global input of each `*SUBMODEL, INPUT=NAME.frd` of the text: the file `NAME.inp` in the
/// folder of the input. `NAME.frd` is the result file of the global model. CalculiX reads it in
/// the folder where it runs, so `fh` solves `NAME.inp` in that folder first.
pub fn global_inputs(text: &str) -> Result<Vec<PathBuf>, String> {
    let mut inputs = Vec::new();
    for name in text.lines().filter_map(|l| input_of(l, "SUBMODEL")) {
        let path = Path::new(name);
        let plain = path.file_name() == Some(path.as_os_str());
        if !plain || path.extension().is_none_or(|e| e != "frd") {
            return Err(format!(
                "*SUBMODEL, INPUT={name}: name the result file of the global model in the \
                 folder of the input, for example INPUT=global.frd"
            ));
        }
        let input = path.with_extension("inp");
        if !inputs.contains(&input) {
            inputs.push(input);
        }
    }
    Ok(inputs)
}

/// Check that an include path stays inside the folder of the input. The path is relative to
/// that folder. CalculiX reads it relative to the folder where it runs, and `fh` runs it in a
/// copy of the folder, so an absolute path or a `..` would leave the copy.
pub fn include_path(name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    let inside = !name.is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if inside {
        Ok(path.to_owned())
    } else {
        Err(format!(
            "*INCLUDE, INPUT={name}: write a relative path with no '..', inside the folder of the input"
        ))
    }
}

const DEPTH: usize = 8;

/// An input with the text of its `*INCLUDE` files in the place of their lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flat {
    pub text: String,
    /// The include files, as paths relative to the folder of the input, in the order read.
    pub files: Vec<PathBuf>,
}

/// Read the input file and put the text of each `*INCLUDE` file in the place of its line.
/// An include path is relative to the folder of the input, also in an included file.
pub fn flatten(input: &Path) -> Result<Flat, String> {
    let dir = input.parent().unwrap_or(Path::new("."));
    let mut flat = Flat::default();
    add(&mut flat, input, dir, 0)?;
    Ok(flat)
}

fn add(flat: &mut Flat, file: &Path, dir: &Path, depth: usize) -> Result<(), String> {
    if depth > DEPTH {
        return Err(format!(
            "*INCLUDE nests deeper than {DEPTH} files at {}",
            file.display()
        ));
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    for line in text.lines() {
        match include_of(line) {
            Some(name) => {
                let relative = include_path(name)?;
                add(flat, &dir.join(&relative), dir, depth + 1)?;
                if !flat.files.contains(&relative) {
                    flat.files.push(relative);
                }
            }
            None => {
                flat.text.push_str(line);
                flat.text.push('\n');
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_submodel_names_the_global_input_beside_it() {
        let text = "*NODE\n*SUBMODEL, TYPE=NODE, input=global.frd\nCUT\n**SUBMODEL, INPUT=x.frd\n";
        assert_eq!(global_inputs(text), Ok(vec![PathBuf::from("global.inp")]));
        for bad in [
            "g/global.frd",
            "../global.frd",
            "/tmp/global.frd",
            "global.dat",
            "global",
        ] {
            let text = format!("*SUBMODEL, TYPE=NODE, INPUT={bad}\nCUT\n");
            assert!(global_inputs(&text).is_err(), "{bad}");
        }
    }

    #[test]
    fn include_paths_stay_inside() {
        assert!(include_path("mesh.inp").is_ok());
        assert!(include_path("./a/mesh.inp").is_ok());
        for bad in ["../mesh.inp", "/etc/mesh.inp", "a/../../x", ""] {
            assert!(include_path(bad).is_err(), "{bad}");
        }
    }
}
