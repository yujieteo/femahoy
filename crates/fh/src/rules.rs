//! Mesh rules of `fh mesh`.

use fh::msh::MshMesh;

/// Refuse a shell mesh from a file that has a volume. A shell needs the mid-surface, and
/// `fh mesh` does not make it from a solid.
pub fn no_solid(mesh: &MshMesh) -> Result<(), String> {
    if mesh.has_volume {
        return Err("the file has a volume, but --shell needs surfaces. fh mesh does not make the mid-surface of a solid. Draw the mid-surface".into());
    }
    Ok(())
}

/// Refuse a mesh with a volume (a surface for a shell) outside every physical group. gmsh writes
/// no element of such a body, so a part is lost without an error.
pub fn volumes_are_named(mesh: &MshMesh, shell: bool) -> Result<(), String> {
    let body = if shell { "surface" } else { "volume" };
    match mesh.unnamed_volumes.as_slice() {
        [] => Ok(()),
        tags => Err(format!(
            "{body} {tags:?} has no physical group, so gmsh wrote none of its elements. Put every {body} in a physical group, or in a --volume box"
        )),
    }
}

/// Refuse a `--face` that selects no face (no edge for a shell), or one that is not on the
/// outside of the part.
pub fn face_surface(mesh: &MshMesh, name: &str, shell: bool) -> Result<(), String> {
    let side = if shell { "edge" } else { "face" };
    let inner = mesh
        .inner_surfaces
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name));
    if inner {
        return Err(format!(
            "{side} {name} is not on the outside of the part. A {side} must be on the outside of the part. For a plane inside the part, use --nodes"
        ));
    }
    match mesh.model.element_surface(name) {
        Some(_) => Ok(()),
        None => Err(format!(
            "the mesh has no {side} group {name}. The selection holds no whole {side} of the part. Check the plane or the box of --face {name}"
        )),
    }
}

/// With `--volume` boxes, each element must be in exactly one box.
pub fn volume_coverage(mesh: &MshMesh, names: &[&str]) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }
    let mut counts = std::collections::HashMap::<u32, usize>::new();
    for name in names {
        let group = mesh
            .model
            .elset(name)
            .filter(|s| !s.ids.is_empty())
            .ok_or_else(|| format!("--volume {name} holds no element"))?;
        for id in &group.ids {
            *counts.entry(*id).or_default() += 1;
        }
    }
    if mesh
        .model
        .elements
        .iter()
        .any(|e| counts.get(&e.id) != Some(&1))
    {
        return Err("each element must be in exactly one --volume box".into());
    }
    Ok(())
}
