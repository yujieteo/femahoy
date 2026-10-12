use fh_io::vtu::{read_pvd, read_vtu, write_pvd, write_vtu, Field, Grid, PvdEntry, VTK_HEXAHEDRON};

fn sample() -> Grid {
    let points = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    Grid {
        cell_types: vec![VTK_HEXAHEDRON],
        cells: vec![(0..8).collect()],
        point_data: vec![
            Field::f64(
                "U",
                3,
                (0..24).map(|i| f64::from(i) * 0.125 - 1.0).collect(),
            ),
            Field::i32("fixed", vec![1, 1, 0, 0, 1, 1, 0, 0]),
        ],
        cell_data: vec![Field::f64("S", 6, vec![1.5, -2.0, 0.0, 3.25, 0.0, 1e-9])],
        points,
    }
}

#[test]
fn vtu_round_trip_keeps_every_value() {
    let g = sample();
    let bytes = write_vtu(&g).unwrap();
    assert_eq!(read_vtu(&bytes).unwrap(), g);
}

#[test]
fn vtu_is_raw_appended_xml() {
    let bytes = write_vtu(&sample()).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("<AppendedData encoding=\"raw\">"));
    assert!(text.contains("format=\"appended\""));
    assert!(text.contains("type=\"Int32\" Name=\"fixed\""));
}

#[test]
fn field_with_the_wrong_length_is_rejected() {
    let mut g = sample();
    g.cell_data = vec![Field::f64("S", 6, vec![0.0; 5])];
    assert!(write_vtu(&g)
        .unwrap_err()
        .to_string()
        .contains("wrong length"));
}

#[test]
fn truncated_file_is_an_error() {
    let bytes = write_vtu(&sample()).unwrap();
    let cut = bytes.len() - 200;
    assert!(read_vtu(&bytes[..cut]).is_err());
    assert!(read_vtu(b"<VTKFile/>").is_err());
}

#[test]
fn field_names_with_special_characters_survive() {
    let mut g = sample();
    g.point_data.push(Field::f64("a<b&\"c\"", 1, vec![0.0; 8]));
    let back = read_vtu(&write_vtu(&g).unwrap()).unwrap();
    assert!(back.point_field("a<b&\"c\"").is_some());
}

#[test]
fn pvd_round_trip() {
    let e = vec![
        PvdEntry {
            time: 0.5,
            file: "job_step1.vtu".into(),
        },
        PvdEntry {
            time: 2.0,
            file: "job_step2.vtu".into(),
        },
    ];
    let text = write_pvd(&e);
    assert!(text.contains("type=\"Collection\""));
    assert_eq!(read_pvd(&text).unwrap(), e);
    assert!(read_pvd("<VTKFile/>").is_err());
}

/// A writer that remembers the largest single write.
struct Spy {
    bytes: Vec<u8>,
    biggest: usize,
}

impl std::io::Write for Spy {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.biggest = self.biggest.max(buf.len());
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn streaming_writer_matches_write_vtu_and_writes_in_pieces() {
    // 200 000 points is 4.8 MB of coordinates and 1.6 MB of one scalar field.
    let n = 200_000;
    let g = Grid {
        points: (0..n).map(|i| [i as f64, 0.5, -1.0]).collect(),
        point_data: vec![Field::f64("T", 1, (0..n).map(|i| i as f64).collect())],
        ..Grid::default()
    };
    let mut spy = Spy {
        bytes: Vec::new(),
        biggest: 0,
    };
    fh_io::vtu::write_vtu_to(&g, &mut spy).unwrap();
    assert_eq!(spy.bytes, write_vtu(&g).unwrap());
    assert!(spy.biggest <= 1 << 20, "largest write {}", spy.biggest);
    assert_eq!(read_vtu(&spy.bytes).unwrap(), g);
}

#[test]
fn pvd_keeps_step_times_and_units() {
    let e = vec![
        PvdEntry {
            time: 1.0,
            file: "step-1.vtu".into(),
        },
        PvdEntry {
            time: 3.5,
            file: "step-2.vtu".into(),
        },
    ];
    let text = fh_io::vtu::write_pvd_units(&e, "mm-N-s");
    assert!(text.contains("units=\"mm-N-s\""), "{text}");
    assert!(text.contains("timestep=\"3.5\""));
    assert_eq!(read_pvd(&text).unwrap(), e);
    assert!(write_pvd(&e).contains("units=\"unspecified\""));
}
