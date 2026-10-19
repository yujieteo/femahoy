//! The text of an input: its units line and its `*INCLUDE` files.

use std::path::{Component, Path, PathBuf};

/// The units of an input from its line `** units: SI-mm`, or an empty text.
pub fn units(text: &str) -> String {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("** units:"))
        .map_or_else(String::new, |u| u.trim().to_owned())
}

/// The file of a line `*INCLUDE, INPUT=FILE`, as the line writes it.
fn include_of(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix('*')?;
    let mut parts = rest.split(',');
    if !parts.next()?.trim().eq_ignore_ascii_case("INCLUDE") {
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
    fn include_paths_stay_inside() {
        assert!(include_path("mesh.inp").is_ok());
        assert!(include_path("./a/mesh.inp").is_ok());
        for bad in ["../mesh.inp", "/etc/mesh.inp", "a/../../x", ""] {
            assert!(include_path(bad).is_err(), "{bad}");
        }
    }
}
