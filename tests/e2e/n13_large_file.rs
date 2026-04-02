use rs_shapefile::*;

/// Path to the National Land Numerical Information (N13) road data.
/// This file is large (~1.9M records) and only available locally.
const N13_PATH: &str = "/Users/akitenkrad/Documents/workspace/N13-24_5339_SHP/N13-24_5339.shp";

fn n13_available() -> bool {
    std::path::Path::new(N13_PATH).exists()
}

/// Large-file tests are skipped in CI where the data isn't present.
macro_rules! n13_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() {
            if !n13_available() {
                eprintln!("skip: N13 data not found");
                return;
            }
            $body
        }
    };
}

// ---------------------------------------------------------------------------
// Metadata tests
// ---------------------------------------------------------------------------

n13_test!(test_shape_type_is_polyline, {
    let sf = ShapefileReader::open(N13_PATH).unwrap();
    assert_eq!(sf.shape_type(), ShapeType::Polyline);
});

n13_test!(test_record_count, {
    let sf = ShapefileReader::open(N13_PATH).unwrap();
    assert_eq!(sf.len(), 1_943_251);
});

n13_test!(test_bbox_covers_expected_area, {
    let sf = ShapefileReader::open(N13_PATH).unwrap();
    let bb = sf.bbox();
    // Mesh 5339 covers roughly lon 139-140, lat 35.33-36.0
    assert!((bb.x_min - 139.0).abs() < 0.001);
    assert!((bb.x_max - 140.0).abs() < 0.001);
    assert!((bb.y_min - 35.333).abs() < 0.001);
    assert!((bb.y_max - 36.0).abs() < 0.001);
});

n13_test!(test_crs_is_jgd2011, {
    let sf = ShapefileReader::open(N13_PATH).unwrap();
    let crs = sf.crs().unwrap();
    assert_eq!(crs.name(), Some("GCS_JGD_2011"));
    assert!(crs.is_geographic());
});

// ---------------------------------------------------------------------------
// Reading tests
// ---------------------------------------------------------------------------

n13_test!(test_records_limit, {
    let mut sf = ShapefileReader::open(N13_PATH).unwrap();
    let records = sf.records(Some(100)).unwrap();
    assert_eq!(records.len(), 100);
});

n13_test!(test_iter_records_no_oom, {
    let mut sf = ShapefileReader::open(N13_PATH).unwrap();
    let count = sf.iter_records().filter_map(|r| r.ok()).count();
    assert_eq!(count, 1_943_251);
});

// ---------------------------------------------------------------------------
// Spatial filter tests
// ---------------------------------------------------------------------------

n13_test!(test_filter_by_bbox_returns_subset, {
    let mut sf = ShapefileReader::open(N13_PATH).unwrap();
    let total = sf.len();
    let subset = sf
        .filter_by_bbox(&BoundingBox {
            x_min: 139.68,
            y_min: 35.68,
            x_max: 139.72,
            y_max: 35.72,
        })
        .unwrap();
    assert!(!subset.is_empty());
    assert!(subset.len() < total);
});

// ---------------------------------------------------------------------------
// Geometry tests
// ---------------------------------------------------------------------------

n13_test!(test_polyline_geometry_properties, {
    let mut sf = ShapefileReader::open(N13_PATH).unwrap();
    let record = sf.get(0).unwrap().unwrap();
    if let Geometry::Polyline(line) = &record.geometry {
        assert!(line.num_parts() >= 1);
        assert!(line.num_points() >= 2);
        assert!(line.length() >= 0.0);
    } else {
        panic!("expected Polyline geometry");
    }
});

// ---------------------------------------------------------------------------
// Attribute / statistics tests
// ---------------------------------------------------------------------------

n13_test!(test_describe_numeric_field, {
    // N13_005 is the only Numeric field in this dataset
    let mut sf = ShapefileReader::open(N13_PATH).unwrap();
    let stats = sf.describe("N13_005").unwrap();
    assert!(stats.count > 0);
    assert!(stats.min >= 0.0);
});

n13_test!(test_describe_character_returns_error, {
    // N13_002 is a Character (Text) field
    let mut sf = ShapefileReader::open(N13_PATH).unwrap();
    let result = sf.describe("N13_002");
    assert!(matches!(
        result,
        Err(ShapefileError::DescribeOnNonNumericField { .. })
    ));
});

n13_test!(test_filter_by_attribute_in, {
    let mut sf = ShapefileReader::open(N13_PATH).unwrap();
    let matched = sf
        .filter_by_attribute_in(
            "N13_002",
            &[
                AttributeValue::Text("1".to_string()),
                AttributeValue::Text("2".to_string()),
            ],
        )
        .unwrap();
    assert!(!matched.is_empty());
    assert!(matched.iter().all(|r| {
        matches!(
            r.attributes.get("N13_002"),
            Some(AttributeValue::Text(s)) if s == "1" || s == "2"
        )
    }));
});
