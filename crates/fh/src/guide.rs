//! The guides of the agent (`fh guide NAME`): 1 how-to text for each solver and each special
//! analysis. The agent gets only the index of the guides with its instructions, and reads a guide
//! when its job needs it, because a job uses 1 or 2 of them. The guides are in `fh`, so an
//! installed `fh` with no clone of the repository has them.

use std::process::ExitCode;

/// The name and the text of each guide. The first line of the text is `# TITLE`.
pub const GUIDES: &[(&str, &str)] = &[
    ("grid", include_str!("guide/grid.md")),
    ("cfd", include_str!("guide/cfd.md")),
    ("cht", include_str!("guide/cht.md")),
    (
        "electromagnetics",
        include_str!("guide/electromagnetics.md"),
    ),
    ("acoustics", include_str!("guide/acoustics.md")),
    ("fsi", include_str!("guide/fsi.md")),
    ("topology", include_str!("guide/topology.md")),
    (
        "shape-optimization",
        include_str!("guide/shape-optimization.md"),
    ),
    ("multibody", include_str!("guide/multibody.md")),
    ("particles", include_str!("guide/particles.md")),
    ("impact", include_str!("guide/impact.md")),
    ("optics", include_str!("guide/optics.md")),
    ("antennas", include_str!("guide/antennas.md")),
    ("shells", include_str!("guide/shells.md")),
    ("composites", include_str!("guide/composites.md")),
    ("cracks", include_str!("guide/cracks.md")),
    ("submodels", include_str!("guide/submodels.md")),
    ("rubber", include_str!("guide/rubber.md")),
    (
        "random-vibration",
        include_str!("guide/random-vibration.md"),
    ),
    ("cfd-report", include_str!("guide/cfd-report.md")),
    ("live-page", include_str!("guide/live-page.md")),
];

/// The title of a guide: its first line with no `# `.
fn title(text: &str) -> &str {
    let line = text.lines().next().unwrap_or_default();
    line.strip_prefix("# ").unwrap_or(line)
}

/// 1 line `- NAME: TITLE` for each guide.
pub fn index() -> String {
    GUIDES
        .iter()
        .map(|(name, text)| format!("- {name}: {}\n", title(text)))
        .collect()
}

/// `fh guide NAME` prints the guide NAME.
pub fn command(args: &[String]) -> Result<ExitCode, String> {
    match args {
        [name] => match GUIDES.iter().find(|(n, _)| n == name) {
            Some((_, text)) => print!("{text}"),
            None => return Err(format!("no guide {name}. The guides:\n{}", index())),
        },
        _ => return Err("usage: fh guide NAME".into()),
    }
    Ok(ExitCode::SUCCESS)
}
