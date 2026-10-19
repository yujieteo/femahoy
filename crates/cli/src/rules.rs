//! Mesh rules of `fh mesh`.

use fh_io::MshMesh;

/// Refuse a mesh with a volume outside every physical group. gmsh writes no element of such a
/// volume, so a part of the body is lost without an error.
pub fn volumes_are_named(mesh: &MshMesh) -> Result<(), String> {
    match mesh.unnamed_volumes.as_slice() {
        [] => Ok(()),
        tags => Err(format!(
            "mesh.volume.unnamed: volume {tags:?} has no physical group, so gmsh wrote none of its elements. Put every volume in a physical group, or in a --volume box"
        )),
    }
}

/// Refuse a `--face` that selects no face, or a face that is not on the outside of the solid.
pub fn face_surface(mesh: &MshMesh, name: &str) -> Result<(), String> {
    let inner = mesh
        .inner_surfaces
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name));
    if inner {
        return Err(format!(
            "mesh.face.inner: face {name} is not on the outside of the solid. A face must be on the outside of the part. For a plane inside the part, use --nodes"
        ));
    }
    match mesh.model.element_surface(name) {
        Some(_) => Ok(()),
        None => Err(format!(
            "mesh.face.missing: the mesh has no face group {name}. The selection holds no whole face of the part. Check the plane or the box of --face {name}"
        )),
    }
}

/// With `--volume` boxes, each solid element must be in exactly one box.
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
            .ok_or_else(|| format!("mesh.volume.empty: --volume {name} holds no solid element"))?;
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
        return Err(
            "mesh.volume.coverage: each solid element must be in exactly one --volume box".into(),
        );
    }
    Ok(())
}
