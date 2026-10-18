//! Mesh rules of the product. Decision D-19, see DESIGN.md: `C3D10` is the default element,
//! and hex8 (`C3D8I`, `C3D8`) is only for an extruded part.

use fh_inp::{ElementSurface, Model};
use fh_io::MshMesh;

/// How far a lateral edge may leave the extrusion direction: the sine of the angle.
const PARALLEL: f64 = 1e-6;

fn is_hex8(element_type: &str) -> bool {
    matches!(element_type, "C3D8I" | "C3D8")
}

/// Refuse a mesh that has a hex8 element and is not one extrusion.
///
/// An extruded hex mesh has one direction. In each hex, the 4 edges from node `i` to node
/// `i + 4` (the lateral edges of the layer) are parallel to that direction.
/// `Err` holds the refusal code and text.
pub fn hex_needs_extrusion(model: &Model) -> Result<(), String> {
    let coords: std::collections::HashMap<u32, [f64; 3]> =
        model.nodes.iter().map(|n| (n.id, n.xyz)).collect();
    let mut direction: Option<[f64; 3]> = None;
    for e in model.elements.iter().filter(|e| is_hex8(&e.element_type)) {
        let refuse = |why: &str| {
            format!(
                "mesh.hex.not_extruded: element {} ({}) {why}. Use C3D10 (D-19), or mesh a 2D shape extruded along one axis",
                e.id, e.element_type
            )
        };
        if e.nodes.len() != 8 {
            return Err(refuse("does not have 8 nodes"));
        }
        for i in 0..4 {
            let (Some(a), Some(b)) = (coords.get(&e.nodes[i]), coords.get(&e.nodes[i + 4])) else {
                return Err(refuse("uses a node that is not in the mesh"));
            };
            let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if length == 0.0 || !length.is_finite() {
                return Err(refuse("has a lateral edge of zero length"));
            }
            let Some(d) = direction else {
                direction = Some(v.map(|x| x / length));
                continue;
            };
            let cross = [
                d[1] * v[2] - d[2] * v[1],
                d[2] * v[0] - d[0] * v[2],
                d[0] * v[1] - d[1] * v[0],
            ];
            let sine =
                (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt() / length;
            if sine > PARALLEL {
                return Err(refuse(
                    "has a lateral edge that is not along the extrusion direction",
                ));
            }
        }
    }
    Ok(())
}

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
    if mesh
        .inner_surfaces
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name))
    {
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

#[cfg(test)]
mod tests {
    use super::*;
    use fh_inp::{Element, Node};

    fn model(kind: &str, shear: f64) -> Model {
        let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let mut m = Model::default();
        for (layer, z) in [0.0, 2.0].into_iter().enumerate() {
            for (i, c) in corners.iter().enumerate() {
                // The shear moves only the top layer corner 2 sideways: a general hex.
                let x = c[0] + if layer == 1 && i == 2 { shear } else { 0.0 };
                m.nodes.push(Node {
                    id: (layer * 4 + i + 1) as u32,
                    xyz: [x, c[1], z],
                });
            }
        }
        m.elements.push(Element {
            id: 1,
            element_type: kind.into(),
            nodes: (1..=8).collect(),
        });
        m
    }

    #[test]
    fn an_extruded_hex_is_accepted() {
        assert_eq!(hex_needs_extrusion(&model("C3D8I", 0.0)), Ok(()));
        assert_eq!(hex_needs_extrusion(&model("C3D8", 0.0)), Ok(()));
    }

    #[test]
    fn a_general_hex_is_refused() {
        for kind in ["C3D8I", "C3D8"] {
            let e = hex_needs_extrusion(&model(kind, 0.3)).unwrap_err();
            assert!(e.starts_with("mesh.hex.not_extruded"), "{e}");
        }
    }

    #[test]
    fn a_tet10_mesh_has_no_rule() {
        assert_eq!(hex_needs_extrusion(&model("C3D10", 0.3)), Ok(()));
    }
}
