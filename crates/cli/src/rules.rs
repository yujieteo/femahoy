//! Mesh rules of the product. Decision D-19, see DESIGN.md: `C3D10` is the only element.

use fh_inp::ElementSurface;
use fh_io::MshMesh;

/// Refuse a mesh with a volume outside every physical group. gmsh writes no element of such a
/// volume, so a part of the body is lost without an error (M2b).
pub fn volumes_are_named(mesh: &MshMesh) -> Result<(), String> {
    match mesh.unnamed_volumes.as_slice() {
        [] => Ok(()),
        tags => Err(format!(
            "mesh.volume.unnamed: volume {tags:?} has no physical group, so gmsh wrote none of its elements. Put every volume in a physical group, or mesh with fh mesh --case"
        )),
    }
}

/// The element-face surface of a region. Refuse a region that the mesh does not hold, or that
/// is not on the outside of the solid.
pub fn region_surface<'m>(mesh: &'m MshMesh, name: &str) -> Result<&'m ElementSurface, String> {
    let inner = mesh
        .inner_surfaces
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name));
    if inner {
        return Err(format!(
            "mesh.region.inner: region {name} is not on the outside of the solid. A load or support region must be a face of the part"
        ));
    }
    mesh.model.element_surface(name).ok_or_else(|| {
        format!(
            "mesh.region.missing: the mesh has no face group {name}. The region selects no face of the part, or the group was lost or renamed. Check the region in case.toml and mesh again with fh mesh --case"
        )
    })
}

/// Each solid element must have exactly one material group.
pub fn material_groups(
    mesh: &fh_io::MshMesh,
    materials: &[fh_validation::MaterialSpec],
) -> Result<(), String> {
    if !materials.iter().any(|m| m.bounds.is_some()) {
        return Ok(());
    }
    let mut counts = std::collections::HashMap::<u32, usize>::new();
    for m in materials {
        let group = mesh
            .model
            .elset(&m.name)
            .filter(|s| !s.ids.is_empty())
            .ok_or_else(|| format!("mesh.material.missing: {} has no solid elements", m.name))?;
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
        return Err("mesh.material.coverage: each solid element needs exactly one material".into());
    }
    Ok(())
}
