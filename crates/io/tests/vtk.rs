//! Legacy VTK (ASCII unstructured grid) writer.

use fh_io::vtk::write_vtk_to;
use fh_io::vtu::{Field, Grid, Values, VTK_HEXAHEDRON};

fn cube() -> Grid {
    let mut points = Vec::new();
    for k in 0..2 {
        for j in 0..2 {
            for i in 0..2 {
                points.push([f64::from(i), f64::from(j), f64::from(k)]);
            }
        }
    }
    Grid {
        points,
        cell_types: vec![VTK_HEXAHEDRON],
        cells: vec![vec![0, 1, 3, 2, 4, 5, 7, 6]],
        point_data: vec![
            Field::i32("NODE_ID", (1..=8).collect()),
            Field::f64("U", 3, (0..24).map(|v| f64::from(v) * 1.5e-4).collect()),
        ],
        cell_data: vec![Field::f64("S", 6, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])],
    }
}

fn text(grid: &Grid) -> String {
    let mut out = Vec::new();
    write_vtk_to(grid, "test title", &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn header_and_geometry_follow_the_legacy_format() {
    let t = text(&cube());
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[0], "# vtk DataFile Version 3.0");
    assert_eq!(lines[1], "test title");
    assert_eq!(lines[2], "ASCII");
    assert_eq!(lines[3], "DATASET UNSTRUCTURED_GRID");
    assert_eq!(lines[4], "POINTS 8 double");
    assert_eq!(lines[5], "0 0 0");
    assert!(t.contains("CELLS 1 9\n8 0 1 3 2 4 5 7 6\n"), "{t}");
    assert!(t.contains("CELL_TYPES 1\n12\n"), "{t}");
}

#[test]
fn fields_are_written_with_their_kind() {
    let t = text(&cube());
    assert!(t.contains("POINT_DATA 8\n"), "{t}");
    assert!(
        t.contains("SCALARS NODE_ID int 1\nLOOKUP_TABLE default\n1\n"),
        "{t}"
    );
    assert!(t.contains("VECTORS U double\n0 0.00015 0.0003\n"), "{t}");
    assert!(t.contains("CELL_DATA 1\n"), "{t}");
    assert!(
        t.contains("FIELD FieldData 1\nS 6 1 double\n1 2 3 4 5 6\n"),
        "{t}"
    );
}

#[test]
fn doubles_survive_a_round_trip() {
    let mut g = cube();
    g.point_data[1] = Field::f64(
        "U",
        3,
        (0..24).map(|v| 1.0 / (3.0 + f64::from(v))).collect(),
    );
    let t = text(&g);
    let at = t.find("VECTORS U double\n").unwrap() + "VECTORS U double\n".len();
    let first: Vec<f64> = t[at..]
        .lines()
        .take(8)
        .flat_map(|l| l.split(' ').map(|v| v.parse::<f64>().unwrap()))
        .collect();
    for (i, v) in first.iter().enumerate() {
        assert_eq!(*v, 1.0 / (3.0 + i as f64));
    }
}

#[test]
fn a_field_of_the_wrong_length_is_refused() {
    let mut g = cube();
    g.point_data[1] = Field::f64("U", 3, vec![0.0; 6]);
    let mut out = Vec::new();
    assert!(write_vtk_to(&g, "t", &mut out).is_err());
}

#[test]
fn a_field_name_with_a_space_or_a_percent_sign_is_encoded() {
    let mut g = cube();
    g.point_data.push(Field::i32("NSET_FIXED FACE", vec![1; 8]));
    g.cell_data.push(Field::f64("100% load\ttab", 1, vec![0.0]));
    let t = text(&g);
    assert!(t.contains("SCALARS NSET_FIXED%20FACE int 1\n"), "{t}");
    assert!(t.contains("SCALARS 100%25%20load%09tab double 1\n"), "{t}");
}

#[test]
fn a_huge_component_count_is_an_error_not_a_panic() {
    let mut g = cube();
    g.point_data.push(Field::f64("big", usize::MAX, vec![]));
    let mut out = Vec::new();
    assert!(write_vtk_to(&g, "t", &mut out).is_err());
}

#[test]
fn a_cell_with_a_point_that_does_not_exist_is_refused() {
    let mut g = cube();
    g.cells[0][0] = 999;
    let mut out = Vec::new();
    assert!(write_vtk_to(&g, "t", &mut out).is_err());
}

#[test]
fn an_integer_field_keeps_its_component_count() {
    let mut g = cube();
    g.point_data.push(Field {
        name: "pairs".into(),
        components: 2,
        values: Values::I32((0..16).collect()),
    });
    let t = text(&g);
    assert!(
        t.contains("FIELD FieldData 1\npairs 2 8 int\n0 1\n2 3\n"),
        "{t}"
    );
}
