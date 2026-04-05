//! Integration tests for GeoJsonReader.

use rs_shapefile::geojson_reader::GeoJsonReader;
use rs_shapefile::{AttributeValue, BoundingBox, Geometry};
use std::io::Write;

fn sample_feature_collection() -> &'static str {
    r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [1.0, 2.0]},
                "properties": {"name": "A", "pop": 100.0}
            },
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [3.0, 4.0]},
                "properties": {"name": "B", "pop": 200.0}
            },
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [5.0, 6.0]},
                "properties": {"name": "C", "pop": 300.0}
            },
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [7.0, 8.0]},
                "properties": {"name": "AB_extra", "pop": 400.0}
            },
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [9.0, 10.0]},
                "properties": {"name": "D", "pop": 500.0}
            },
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [11.0, 12.0]},
                "properties": {"name": "E", "pop": 600.0}
            }
        ]
    }"#
}

#[test]
fn test_open_geojson_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.geojson");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(sample_feature_collection().as_bytes()).unwrap();
    }
    let reader = GeoJsonReader::open(&path).unwrap();
    assert_eq!(reader.len(), 6);
}

#[test]
fn test_from_str() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    assert_eq!(reader.len(), 6);
    assert!(!reader.is_empty());
}

#[test]
fn test_len_and_bbox() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    assert_eq!(reader.len(), 6);
    let bb = reader.bbox();
    assert!((bb.x_min - 1.0).abs() < 1e-10);
    assert!((bb.y_min - 2.0).abs() < 1e-10);
    assert!((bb.x_max - 11.0).abs() < 1e-10);
    assert!((bb.y_max - 12.0).abs() < 1e-10);
}

#[test]
fn test_records_limit() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    let recs = reader.records(Some(5));
    assert_eq!(recs.len(), 5);
    let all = reader.records(None);
    assert_eq!(all.len(), 6);
}

#[test]
fn test_iter_records() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    let count = reader.iter_records().count();
    assert_eq!(count, 6);
}

#[test]
fn test_get_by_index() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    assert!(reader.get(0).is_some());
    assert!(reader.get(5).is_some());
    assert!(reader.get(9999).is_none());
}

#[test]
fn test_filter_by_attribute() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    let results = reader.filter_by_attribute("name", &AttributeValue::Text("B".to_string()));
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].attributes.get("name"),
        Some(&AttributeValue::Text("B".to_string()))
    );
}

#[test]
fn test_filter_by_bbox() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    let bbox = BoundingBox {
        x_min: 0.0,
        y_min: 0.0,
        x_max: 4.0,
        y_max: 5.0,
    };
    let results = reader.filter_by_bbox(&bbox);
    // Points (1,2) and (3,4) are within this bbox
    assert_eq!(results.len(), 2);
}

#[test]
fn test_describe_numeric() {
    let reader = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    let stats = reader.describe("pop").unwrap();
    assert_eq!(stats.count, 6);
    assert!((stats.min - 100.0).abs() < 1e-10);
    assert!((stats.max - 600.0).abs() < 1e-10);
    assert!((stats.mean - 350.0).abs() < 1e-10);
    // Median of [100, 200, 300, 400, 500, 600] = (300+400)/2 = 350
    assert!((stats.median - 350.0).abs() < 1e-10);
}

#[test]
fn test_to_geojson_roundtrip() {
    let reader1 = GeoJsonReader::from_str(sample_feature_collection()).unwrap();
    let exported = reader1.to_geojson(None).unwrap();
    let reader2 = GeoJsonReader::from_str(&exported).unwrap();
    assert_eq!(reader1.len(), reader2.len());
    // Verify bbox matches
    assert!((reader1.bbox().x_min - reader2.bbox().x_min).abs() < 1e-10);
    assert!((reader1.bbox().y_max - reader2.bbox().y_max).abs() < 1e-10);
}

#[test]
fn test_mixed_geometry_types() {
    let json_str = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "geometry": {"type": "Point", "coordinates": [1.0, 2.0]},
                "properties": {"type": "point"}
            },
            {
                "type": "Feature",
                "geometry": {"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]},
                "properties": {"type": "line"}
            }
        ]
    }"#;
    let reader = GeoJsonReader::from_str(json_str).unwrap();
    assert_eq!(reader.len(), 2);

    let rec0 = reader.get(0).unwrap();
    assert!(matches!(rec0.geometry, Geometry::Point(_)));

    let rec1 = reader.get(1).unwrap();
    assert!(matches!(rec1.geometry, Geometry::Polyline(_)));
}

#[test]
fn test_3d_coordinates_ignored() {
    let json_str = r#"{
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [1.0, 2.0, 999.0]},
        "properties": {}
    }"#;
    let reader = GeoJsonReader::from_str(json_str).unwrap();
    let rec = reader.get(0).unwrap();
    if let Geometry::Point(p) = &rec.geometry {
        assert!((p.x - 1.0).abs() < 1e-10);
        assert!((p.y - 2.0).abs() < 1e-10);
        // z is ignored — only x, y stored
    } else {
        panic!("expected Point geometry");
    }
}
