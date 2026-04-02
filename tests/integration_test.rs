mod helpers;

use helpers::*;
use rs_shapefile::*;
use tempfile::TempDir;

/// Create a test shapefile with 3 polyline records, each having a NAME (C,20) and VALUE (N,10,2) field.
fn create_polyline_test_files() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();

    // Build 3 polyline records for .shp
    let rec1 = polyline_record_bytes(1, &[(0.0, 0.0), (1.0, 1.0)]);
    let rec2 = polyline_record_bytes(2, &[(10.0, 10.0), (20.0, 20.0)]);
    let rec3 = polyline_record_bytes(3, &[(50.0, 50.0), (60.0, 60.0)]);

    let shp_bytes = build_shp_file(3, &[rec1.clone(), rec2.clone(), rec3.clone()]);

    // Build .shx from the record offsets
    // Each polyline record with 2 points: record header(8) + shape(4) + bbox(32) + nparts(4) + npts(4) + parts(4) + points(32) = 88 bytes
    // content_length = 88 - 8 = 80 bytes
    let rec_len = 88u32;
    let content_len = 80u32;
    let shx_bytes = build_shx_file(
        3,
        &[
            (100, content_len),
            (100 + rec_len as u64, content_len),
            (100 + 2 * rec_len as u64, content_len),
        ],
    );

    // Build .dbf with NAME and VALUE fields
    let dbf_bytes = build_dbf_file(
        &[("NAME", b'C', 20, 0), ("VALUE", b'N', 10, 2)],
        &[
            (0x20, &[pad_right("Tokyo", 20), pad_left("100.50", 10)]),
            (0x20, &[pad_right("Osaka", 20), pad_left("200.75", 10)]),
            (0x20, &[pad_right("Nagoya", 20), pad_left("300.25", 10)]),
        ],
    );

    let shp_path = write_test_shapefile(
        dir.path(),
        "test",
        &shp_bytes,
        &dbf_bytes,
        Some(&shx_bytes),
        None,
    );

    (dir, shp_path)
}

/// Create a test shapefile with point records for bbox testing.
/// Points at (5,5), (15,15), (50,50)
fn create_point_test_files() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();

    let rec1 = point_record_bytes(1, 5.0, 5.0);
    let rec2 = point_record_bytes(2, 15.0, 15.0);
    let rec3 = point_record_bytes(3, 50.0, 50.0);

    let shp_bytes = build_shp_file(1, &[rec1.clone(), rec2.clone(), rec3.clone()]);

    // Point record: record header(8) + shape_type(4) + x(8) + y(8) = 28 bytes
    // content_length = 20 bytes
    let rec_len = 28u32;
    let content_len = 20u32;
    let shx_bytes = build_shx_file(
        1,
        &[
            (100, content_len),
            (100 + rec_len as u64, content_len),
            (100 + 2 * rec_len as u64, content_len),
        ],
    );

    let dbf_bytes = build_dbf_file(
        &[("NAME", b'C', 20, 0), ("VALUE", b'N', 10, 2)],
        &[
            (0x20, &[pad_right("Tokyo", 20), pad_left("100.50", 10)]),
            (0x20, &[pad_right("Osaka", 20), pad_left("200.75", 10)]),
            (0x20, &[pad_right("Nagoya", 20), pad_left("300.25", 10)]),
        ],
    );

    let shp_path = write_test_shapefile(
        dir.path(),
        "test",
        &shp_bytes,
        &dbf_bytes,
        Some(&shx_bytes),
        None,
    );

    (dir, shp_path)
}

#[test]
fn test_open_polyline_shapefile() {
    let (_dir, shp_path) = create_polyline_test_files();
    let reader = ShapefileReader::open(&shp_path);
    assert!(
        reader.is_ok(),
        "ShapefileReader::open should succeed: {:?}",
        reader.err()
    );
}

#[test]
fn test_shape_type_polyline() {
    let (_dir, shp_path) = create_polyline_test_files();
    let reader = ShapefileReader::open(&shp_path).unwrap();
    assert_eq!(reader.shape_type(), ShapeType::Polyline);
}

#[test]
fn test_records_count() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let records = reader.records(None).unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(reader.len(), 3);
}

#[test]
fn test_records_limit() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let records = reader.records(Some(2)).unwrap();
    assert_eq!(records.len(), 2);
}

#[test]
fn test_iter_records_matches_records() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();

    let records = reader.records(None).unwrap();
    let iter_records: Vec<ShapeRecord> = reader
        .iter_records()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(records.len(), iter_records.len());
    for (a, b) in records.iter().zip(iter_records.iter()) {
        assert_eq!(a.record_number, b.record_number);
        assert_eq!(a.geometry, b.geometry);
        assert_eq!(a.attributes, b.attributes);
    }
}

#[test]
fn test_get_first_record() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let record = reader.get(0).unwrap();
    assert!(record.is_some());
    let record = record.unwrap();
    assert_eq!(record.record_number, 1);
    assert_eq!(
        record.attributes.get("NAME"),
        Some(&AttributeValue::Text("Tokyo".to_string()))
    );
}

#[test]
fn test_get_out_of_range() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let record = reader.get(9999).unwrap();
    assert!(record.is_none());
}

#[test]
fn test_filter_by_attribute_exact() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let results = reader
        .filter_by_attribute("NAME", &AttributeValue::Text("Osaka".to_string()))
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].attributes.get("NAME"),
        Some(&AttributeValue::Text("Osaka".to_string()))
    );
}

#[test]
fn test_filter_by_attribute_in() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let results = reader
        .filter_by_attribute_in(
            "NAME",
            &[
                AttributeValue::Text("Tokyo".to_string()),
                AttributeValue::Text("Nagoya".to_string()),
            ],
        )
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn test_filter_by_attribute_starts_with() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    // "Tokyo" and "Tottori" would match "To" — but we only have "Tokyo"
    let results = reader
        .filter_by_attribute_starts_with("NAME", "Na")
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].attributes.get("NAME"),
        Some(&AttributeValue::Text("Nagoya".to_string()))
    );
}

#[test]
fn test_filter_by_bbox() {
    let (_dir, shp_path) = create_point_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();

    // BBox that covers (0,0)-(20,20) should match points at (5,5) and (15,15)
    let query_bbox = BoundingBox {
        x_min: 0.0,
        y_min: 0.0,
        x_max: 20.0,
        y_max: 20.0,
    };
    let results = reader.filter_by_bbox(&query_bbox).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn test_describe_numeric() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let stats = reader.describe("VALUE").unwrap();
    assert_eq!(stats.count, 3);
    assert!((stats.min - 100.50).abs() < 0.01);
    assert!((stats.max - 300.25).abs() < 0.01);
    // mean = (100.50 + 200.75 + 300.25) / 3 = 200.5
    assert!((stats.mean - 200.5).abs() < 0.01);
    // median = 200.75 (middle value)
    assert!((stats.median - 200.75).abs() < 0.01);
}

#[test]
fn test_describe_non_numeric_returns_error() {
    let (_dir, shp_path) = create_polyline_test_files();
    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    let result = reader.describe("NAME");
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ShapefileError::DescribeOnNonNumericField { .. }
    ));
}

#[test]
fn test_missing_shp_returns_error() {
    let dir = TempDir::new().unwrap();
    let nonexistent = dir.path().join("nonexistent.shp");
    let result = ShapefileReader::open(&nonexistent);
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        matches!(err, ShapefileError::MissingFile { .. }),
        "expected MissingFile, got: {err:?}"
    );
}

#[test]
fn test_missing_dbf_returns_error() {
    let dir = TempDir::new().unwrap();
    // Create .shp but not .dbf
    let shp_path = dir.path().join("test.shp");
    let shp_bytes = build_shp_file(3, &[]);
    std::fs::write(&shp_path, shp_bytes).unwrap();

    let result = ShapefileReader::open(&shp_path);
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        matches!(err, ShapefileError::MissingFile { .. }),
        "expected MissingFile, got: {err:?}"
    );
}

#[test]
fn test_missing_shx_fallback() {
    let dir = TempDir::new().unwrap();

    let rec1 = polyline_record_bytes(1, &[(0.0, 0.0), (1.0, 1.0)]);
    let shp_bytes = build_shp_file(3, &[rec1]);

    let dbf_bytes = build_dbf_file(
        &[("NAME", b'C', 10, 0)],
        &[(0x20, &[pad_right("Test", 10)])],
    );

    // Write .shp and .dbf but NOT .shx
    let shp_path = write_test_shapefile(
        dir.path(),
        "test",
        &shp_bytes,
        &dbf_bytes,
        None, // no .shx
        None,
    );

    let mut reader = ShapefileReader::open(&shp_path).unwrap();
    assert_eq!(reader.len(), 1);
    let records = reader.records(None).unwrap();
    assert_eq!(records.len(), 1);
}

#[test]
fn test_crs_is_geographic() {
    let dir = TempDir::new().unwrap();

    let rec1 = polyline_record_bytes(1, &[(139.0, 35.0), (140.0, 36.0)]);
    let shp_bytes = build_shp_file(3, &[rec1]);

    let dbf_bytes = build_dbf_file(
        &[("NAME", b'C', 10, 0)],
        &[(0x20, &[pad_right("Test", 10)])],
    );

    let prj_wkt = r#"GEOGCS["GCS_JGD_2011",DATUM["D_JGD_2011",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#;

    let shp_path = write_test_shapefile(
        dir.path(),
        "test",
        &shp_bytes,
        &dbf_bytes,
        None,
        Some(prj_wkt),
    );

    let reader = ShapefileReader::open(&shp_path).unwrap();
    assert!(reader.crs().is_some());
    assert!(reader.crs().unwrap().is_geographic());
}
