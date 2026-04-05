//! Integration tests for `MvtReader`.
//!
//! Since we may not have real PBF fixture files, all tests build protobuf data
//! programmatically using `prost::Message::encode_to_vec()`.

use prost::Message;
use rs_shapefile::mvt_reader::vector_tile;
use rs_shapefile::{AttributeValue, BoundingBox, Geometry, LayerFilter, MvtReader, TileCoord};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_test_tile(layers: Vec<vector_tile::tile::Layer>) -> Vec<u8> {
    let tile = vector_tile::Tile { layers };
    tile.encode_to_vec()
}

fn encode_command(id: u32, count: u32) -> u32 {
    (id & 0x7) | (count << 3)
}

fn encode_zigzag(n: i32) -> u32 {
    ((n << 1) ^ (n >> 31)) as u32
}

/// Build a layer with a single POINT feature at pixel (100, 200).
fn point_layer(
    name: &str,
    keys: Vec<String>,
    values: Vec<vector_tile::tile::Value>,
    tags: Vec<u32>,
) -> vector_tile::tile::Layer {
    vector_tile::tile::Layer {
        version: 2,
        name: name.to_string(),
        features: vec![vector_tile::tile::Feature {
            id: Some(1),
            tags,
            r#type: Some(1), // POINT
            geometry: vec![encode_command(1, 1), encode_zigzag(100), encode_zigzag(200)],
        }],
        keys,
        values,
        extent: Some(4096),
    }
}

/// Build a basic point layer with a "name" attribute.
fn basic_point_layer(layer_name: &str, attr_value: &str) -> vector_tile::tile::Layer {
    point_layer(
        layer_name,
        vec!["name".to_string()],
        vec![vector_tile::tile::Value {
            string_value: Some(attr_value.to_string()),
            ..Default::default()
        }],
        vec![0, 0],
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_from_bytes_basic() {
    let bytes = build_test_tile(vec![basic_point_layer("test", "point_a")]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    assert_eq!(reader.len(), 1);
    let rec = reader.get(0).unwrap();
    matches!(rec.geometry, Geometry::Point(_));
    assert_eq!(
        rec.attributes.get("name"),
        Some(&AttributeValue::Text("point_a".to_string()))
    );
}

#[test]
fn test_layer_names() {
    let bytes = build_test_tile(vec![
        basic_point_layer("roads", "r1"),
        basic_point_layer("buildings", "b1"),
    ]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    let names = reader.layer_names();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"roads".to_string()));
    assert!(names.contains(&"buildings".to_string()));
}

#[test]
fn test_len_positive() {
    let bytes = build_test_tile(vec![basic_point_layer("test", "a")]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    assert!(reader.len() > 0);
}

#[test]
fn test_bbox_covers_tile() {
    let tc = TileCoord::new(14, 14552, 6451);
    let bytes = build_test_tile(vec![basic_point_layer("test", "a")]);
    let reader = MvtReader::from_bytes(&bytes, tc).unwrap();
    let expected = tc.to_bbox();
    let actual = reader.bbox();
    assert!((actual.x_min - expected.x_min).abs() < 1e-10);
    assert!((actual.y_min - expected.y_min).abs() < 1e-10);
    assert!((actual.x_max - expected.x_max).abs() < 1e-10);
    assert!((actual.y_max - expected.y_max).abs() < 1e-10);
}

#[test]
fn test_filter_by_layer() {
    let bytes = build_test_tile(vec![
        basic_point_layer("roads", "r1"),
        basic_point_layer("buildings", "b1"),
    ]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    let roads = reader.filter_by_layer("roads");
    assert_eq!(roads.len(), 1);
    assert_eq!(
        roads[0].attributes.get("_layer"),
        Some(&AttributeValue::Text("roads".to_string()))
    );
}

#[test]
fn test_filter_by_attribute() {
    let bytes = build_test_tile(vec![basic_point_layer("test", "target")]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    let matches = reader.filter_by_attribute("name", &AttributeValue::Text("target".to_string()));
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_records_limit() {
    let bytes = build_test_tile(vec![
        basic_point_layer("a", "1"),
        basic_point_layer("b", "2"),
        basic_point_layer("c", "3"),
    ]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    assert_eq!(reader.records(Some(1)).len(), 1);
}

#[test]
fn test_iter_records_count() {
    let bytes = build_test_tile(vec![
        basic_point_layer("a", "1"),
        basic_point_layer("b", "2"),
    ]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    assert_eq!(reader.iter_records().count(), reader.len());
}

#[test]
fn test_get_first_record() {
    let bytes = build_test_tile(vec![basic_point_layer("test", "a")]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    assert!(reader.get(0).is_some());
}

#[test]
fn test_get_out_of_range() {
    let bytes = build_test_tile(vec![basic_point_layer("test", "a")]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    assert!(reader.get(9999).is_none());
}

#[test]
fn test_describe_numeric() {
    // Build a layer with a numeric attribute.
    let layer = vector_tile::tile::Layer {
        version: 2,
        name: "test".to_string(),
        features: vec![
            vector_tile::tile::Feature {
                id: Some(1),
                tags: vec![0, 0],
                r#type: Some(1),
                geometry: vec![encode_command(1, 1), encode_zigzag(10), encode_zigzag(20)],
            },
            vector_tile::tile::Feature {
                id: Some(2),
                tags: vec![0, 1],
                r#type: Some(1),
                geometry: vec![encode_command(1, 1), encode_zigzag(30), encode_zigzag(40)],
            },
        ],
        keys: vec!["pop".to_string()],
        values: vec![
            vector_tile::tile::Value {
                int_value: Some(100),
                ..Default::default()
            },
            vector_tile::tile::Value {
                int_value: Some(200),
                ..Default::default()
            },
        ],
        extent: Some(4096),
    };
    let bytes = build_test_tile(vec![layer]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    let stats = reader.describe("pop").unwrap();
    assert_eq!(stats.count, 2);
    assert!((stats.min - 100.0).abs() < 1e-10);
    assert!((stats.max - 200.0).abs() < 1e-10);
    assert!((stats.mean - 150.0).abs() < 1e-10);
}

#[test]
fn test_filter_by_bbox() {
    let tc = TileCoord::new(14, 14552, 6451);
    let bytes = build_test_tile(vec![basic_point_layer("test", "a")]);
    let reader = MvtReader::from_bytes(&bytes, tc).unwrap();
    // Use a bbox that covers the whole tile
    let tile_bbox = tc.to_bbox();
    let matches = reader.filter_by_bbox(&tile_bbox);
    assert_eq!(matches.len(), 1);

    // Use a bbox far away - should match nothing
    let far = BoundingBox {
        x_min: -180.0,
        y_min: -90.0,
        x_max: -170.0,
        y_max: -80.0,
    };
    let no_match = reader.filter_by_bbox(&far);
    assert_eq!(no_match.len(), 0);
}

#[test]
fn test_road_centerline_filter() {
    let layer = vector_tile::tile::Layer {
        version: 2,
        name: "road".to_string(),
        features: vec![
            vector_tile::tile::Feature {
                id: Some(1),
                tags: vec![0, 0], // ftCode = 2701
                r#type: Some(1),
                geometry: vec![encode_command(1, 1), encode_zigzag(10), encode_zigzag(20)],
            },
            vector_tile::tile::Feature {
                id: Some(2),
                tags: vec![0, 1], // ftCode = 9999
                r#type: Some(1),
                geometry: vec![encode_command(1, 1), encode_zigzag(30), encode_zigzag(40)],
            },
        ],
        keys: vec!["ftCode".to_string()],
        values: vec![
            vector_tile::tile::Value {
                int_value: Some(2701),
                ..Default::default()
            },
            vector_tile::tile::Value {
                int_value: Some(9999),
                ..Default::default()
            },
        ],
        extent: Some(4096),
    };
    let bytes = build_test_tile(vec![layer]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    let filter = LayerFilter::road_centerline();
    let matches = reader.filter_by_layer_filter(&filter);
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_linestring_geometry() {
    let layer = vector_tile::tile::Layer {
        version: 2,
        name: "lines".to_string(),
        features: vec![vector_tile::tile::Feature {
            id: Some(1),
            tags: vec![],
            r#type: Some(2), // LINESTRING
            geometry: vec![
                encode_command(1, 1), // MoveTo
                encode_zigzag(0),
                encode_zigzag(0),
                encode_command(2, 2), // LineTo x2
                encode_zigzag(100),
                encode_zigzag(0),
                encode_zigzag(0),
                encode_zigzag(100),
            ],
        }],
        keys: vec![],
        values: vec![],
        extent: Some(4096),
    };
    let bytes = build_test_tile(vec![layer]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    let rec = reader.get(0).unwrap();
    match &rec.geometry {
        Geometry::Polyline(pl) => {
            assert_eq!(pl.parts.len(), 1);
            assert_eq!(pl.parts[0].len(), 3);
        }
        other => panic!("expected Polyline, got {:?}", other),
    }
}

#[test]
fn test_polygon_geometry() {
    // Clockwise exterior triangle in pixel space (Y-down)
    let layer = vector_tile::tile::Layer {
        version: 2,
        name: "polys".to_string(),
        features: vec![vector_tile::tile::Feature {
            id: Some(1),
            tags: vec![],
            r#type: Some(3), // POLYGON
            geometry: vec![
                encode_command(1, 1), // MoveTo (0,0)
                encode_zigzag(0),
                encode_zigzag(0),
                encode_command(2, 2), // LineTo (100,0) then (0,100)
                encode_zigzag(100),
                encode_zigzag(0),
                encode_zigzag(-100),
                encode_zigzag(100),
                encode_command(7, 1), // ClosePath
            ],
        }],
        keys: vec![],
        values: vec![],
        extent: Some(4096),
    };
    let bytes = build_test_tile(vec![layer]);
    let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
    let rec = reader.get(0).unwrap();
    match &rec.geometry {
        Geometry::Polygon(pg) => {
            assert!(!pg.rings.is_empty());
            // Ring should be closed
            let ring = &pg.rings[0];
            assert_eq!(ring.points.first(), ring.points.last());
            // 3 points + closing point = 4
            assert_eq!(ring.points.len(), 4);
        }
        other => panic!("expected Polygon, got {:?}", other),
    }
}
