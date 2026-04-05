//! GeoJSON reader that converts GeoJSON (RFC 7946) into the library's model types.
//!
//! This module is only available when the `geojson` feature is enabled.

#![cfg(feature = "geojson")]

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::error::ShapefileError;
use crate::models::attribute::{AttributeValue, FieldStats};
use crate::models::bbox::BoundingBox;
use crate::models::geometry::{Geometry, MultiPoint, Point, Polygon, Polyline, Ring};
use crate::models::record::ShapeRecord;

// ---------------------------------------------------------------------------
// Internal conversion functions
// ---------------------------------------------------------------------------

fn parse_point(coords: &[Value]) -> Result<Point, ShapefileError> {
    if coords.len() < 2 {
        return Err(ShapefileError::InvalidGeoJson {
            reason: "Point requires at least 2 coordinates".to_string(),
        });
    }
    let x = coords[0]
        .as_f64()
        .ok_or_else(|| ShapefileError::InvalidGeoJson {
            reason: "coordinate is not a number".to_string(),
        })?;
    let y = coords[1]
        .as_f64()
        .ok_or_else(|| ShapefileError::InvalidGeoJson {
            reason: "coordinate is not a number".to_string(),
        })?;
    Ok(Point { x, y })
}

fn parse_linestring(coords: &[Value]) -> Result<Vec<Point>, ShapefileError> {
    coords
        .iter()
        .map(|c| {
            let arr = c.as_array().ok_or_else(|| ShapefileError::InvalidGeoJson {
                reason: "LineString coordinate is not an array".to_string(),
            })?;
            parse_point(arr)
        })
        .collect()
}

fn parse_polygon_rings(coords: &[Value]) -> Result<Vec<Ring>, ShapefileError> {
    coords
        .iter()
        .map(|ring_val| {
            let ring_arr = ring_val
                .as_array()
                .ok_or_else(|| ShapefileError::InvalidGeoJson {
                    reason: "Polygon ring is not an array".to_string(),
                })?;
            let points = parse_linestring(ring_arr)?;
            Ok(Ring { points })
        })
        .collect()
}

fn parse_multipoint(coords: &[Value]) -> Result<MultiPoint, ShapefileError> {
    let points: Vec<Point> = coords
        .iter()
        .map(|c| {
            let arr = c.as_array().ok_or_else(|| ShapefileError::InvalidGeoJson {
                reason: "MultiPoint coordinate is not an array".to_string(),
            })?;
            parse_point(arr)
        })
        .collect::<Result<_, _>>()?;
    Ok(MultiPoint { points })
}

fn convert_geometry(geom: &Value) -> Result<Vec<Geometry>, ShapefileError> {
    if geom.is_null() {
        return Ok(vec![Geometry::Null]);
    }

    let geom_type = geom.get("type").and_then(|t| t.as_str()).ok_or_else(|| {
        ShapefileError::InvalidGeoJson {
            reason: "geometry missing 'type' field".to_string(),
        }
    })?;

    let coords_val = geom.get("coordinates");

    match geom_type {
        "Point" => {
            let coords = coords_val.and_then(|c| c.as_array()).ok_or_else(|| {
                ShapefileError::InvalidGeoJson {
                    reason: "Point missing 'coordinates'".to_string(),
                }
            })?;
            let p = parse_point(coords)?;
            Ok(vec![Geometry::Point(p)])
        }
        "MultiPoint" => {
            let coords = coords_val.and_then(|c| c.as_array()).ok_or_else(|| {
                ShapefileError::InvalidGeoJson {
                    reason: "MultiPoint missing 'coordinates'".to_string(),
                }
            })?;
            let mp = parse_multipoint(coords)?;
            Ok(vec![Geometry::MultiPoint(mp)])
        }
        "LineString" => {
            let coords = coords_val.and_then(|c| c.as_array()).ok_or_else(|| {
                ShapefileError::InvalidGeoJson {
                    reason: "LineString missing 'coordinates'".to_string(),
                }
            })?;
            let points = parse_linestring(coords)?;
            Ok(vec![Geometry::Polyline(Polyline {
                parts: vec![points],
            })])
        }
        "MultiLineString" => {
            let coords = coords_val.and_then(|c| c.as_array()).ok_or_else(|| {
                ShapefileError::InvalidGeoJson {
                    reason: "MultiLineString missing 'coordinates'".to_string(),
                }
            })?;
            let parts: Vec<Vec<Point>> = coords
                .iter()
                .map(|line| {
                    let arr = line
                        .as_array()
                        .ok_or_else(|| ShapefileError::InvalidGeoJson {
                            reason: "MultiLineString part is not an array".to_string(),
                        })?;
                    parse_linestring(arr)
                })
                .collect::<Result<_, _>>()?;
            Ok(vec![Geometry::Polyline(Polyline { parts })])
        }
        "Polygon" => {
            let coords = coords_val.and_then(|c| c.as_array()).ok_or_else(|| {
                ShapefileError::InvalidGeoJson {
                    reason: "Polygon missing 'coordinates'".to_string(),
                }
            })?;
            let rings = parse_polygon_rings(coords)?;
            Ok(vec![Geometry::Polygon(Polygon { rings })])
        }
        "MultiPolygon" => {
            let coords = coords_val.and_then(|c| c.as_array()).ok_or_else(|| {
                ShapefileError::InvalidGeoJson {
                    reason: "MultiPolygon missing 'coordinates'".to_string(),
                }
            })?;
            let polygons: Vec<Geometry> = coords
                .iter()
                .map(|poly_val| {
                    let poly_arr =
                        poly_val
                            .as_array()
                            .ok_or_else(|| ShapefileError::InvalidGeoJson {
                                reason: "MultiPolygon element is not an array".to_string(),
                            })?;
                    let rings = parse_polygon_rings(poly_arr)?;
                    Ok(Geometry::Polygon(Polygon { rings }))
                })
                .collect::<Result<_, ShapefileError>>()?;
            Ok(polygons)
        }
        "GeometryCollection" => Err(ShapefileError::InvalidGeoJson {
            reason: "GeometryCollection is not supported".to_string(),
        }),
        other => Err(ShapefileError::InvalidGeoJson {
            reason: format!("unsupported geometry type: {other}"),
        }),
    }
}

fn convert_properties(props: &Value) -> HashMap<String, AttributeValue> {
    let mut map = HashMap::new();
    if let Some(obj) = props.as_object() {
        for (k, v) in obj {
            let attr = match v {
                Value::String(s) => AttributeValue::Text(s.clone()),
                Value::Number(n) => AttributeValue::Numeric(n.as_f64().unwrap_or(0.0)),
                Value::Bool(b) => AttributeValue::Logical(*b),
                Value::Null => AttributeValue::Null,
                _ => AttributeValue::Text(serde_json::to_string(v).unwrap_or_default()),
            };
            map.insert(k.clone(), attr);
        }
    }
    map
}

fn convert_feature(feature: &Value, start_number: u32) -> Result<Vec<ShapeRecord>, ShapefileError> {
    let geom_val = feature.get("geometry").unwrap_or(&Value::Null);
    let geometries = convert_geometry(geom_val)?;

    let props_val = feature.get("properties").unwrap_or(&Value::Null);
    let attributes = convert_properties(props_val);

    let records: Vec<ShapeRecord> = geometries
        .into_iter()
        .enumerate()
        .map(|(i, geometry)| ShapeRecord {
            record_number: start_number + i as u32,
            geometry,
            attributes: attributes.clone(),
        })
        .collect();

    Ok(records)
}

fn compute_bbox(root: &Value, records: &[ShapeRecord]) -> BoundingBox {
    if let Some(bbox_arr) = root.get("bbox").and_then(|b| b.as_array()) {
        if bbox_arr.len() >= 4 {
            if let (Some(x_min), Some(y_min), Some(x_max), Some(y_max)) = (
                bbox_arr[0].as_f64(),
                bbox_arr[1].as_f64(),
                bbox_arr[2].as_f64(),
                bbox_arr[3].as_f64(),
            ) {
                return BoundingBox {
                    x_min,
                    y_min,
                    x_max,
                    y_max,
                };
            }
        }
    }

    // Compute from records
    let mut x_min = f64::INFINITY;
    let mut y_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_max = f64::NEG_INFINITY;

    for rec in records {
        if let Some(bb) = rec.geometry.bbox() {
            x_min = x_min.min(bb.x_min);
            y_min = y_min.min(bb.y_min);
            x_max = x_max.max(bb.x_max);
            y_max = y_max.max(bb.y_max);
        }
    }

    BoundingBox {
        x_min,
        y_min,
        x_max,
        y_max,
    }
}

// ---------------------------------------------------------------------------
// GeoJsonReader
// ---------------------------------------------------------------------------

/// A reader for GeoJSON (RFC 7946) files that provides the same analysis API
/// as `ShapefileReader`.
///
/// All records are loaded into memory at construction time, so accessor methods
/// return borrowed references rather than owned values.
pub struct GeoJsonReader {
    records: Vec<ShapeRecord>,
    bbox: BoundingBox,
}

impl GeoJsonReader {
    /// Opens a GeoJSON file at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ShapefileError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Parses a GeoJSON string into a `GeoJsonReader`.
    /// Parses a GeoJSON string into a `GeoJsonReader`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(json: &str) -> Result<Self, ShapefileError> {
        let root: Value = serde_json::from_str(json)?;
        Self::from_value(&root)
    }

    /// Reads GeoJSON from any `Read` source.
    pub fn from_reader(mut reader: impl Read) -> Result<Self, ShapefileError> {
        let mut buf = String::new();
        reader.read_to_string(&mut buf)?;
        Self::from_str(&buf)
    }

    fn from_value(root: &Value) -> Result<Self, ShapefileError> {
        let type_str = root.get("type").and_then(|t| t.as_str()).ok_or_else(|| {
            ShapefileError::InvalidGeoJson {
                reason: "missing 'type' field".to_string(),
            }
        })?;

        let mut records = Vec::new();

        match type_str {
            "FeatureCollection" => {
                let features =
                    root.get("features")
                        .and_then(|f| f.as_array())
                        .ok_or_else(|| ShapefileError::InvalidGeoJson {
                            reason: "FeatureCollection missing 'features' array".to_string(),
                        })?;
                let mut next_number: u32 = 1;
                for feature in features {
                    let recs = convert_feature(feature, next_number)?;
                    next_number += recs.len() as u32;
                    records.extend(recs);
                }
            }
            "Feature" => {
                records = convert_feature(root, 1)?;
            }
            "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon"
            | "MultiPolygon" => {
                // Bare geometry — wrap as feature with no properties
                let geometries = convert_geometry(root)?;
                for (i, geom) in geometries.into_iter().enumerate() {
                    records.push(ShapeRecord {
                        record_number: (i + 1) as u32,
                        geometry: geom,
                        attributes: HashMap::new(),
                    });
                }
            }
            other => {
                return Err(ShapefileError::InvalidGeoJson {
                    reason: format!("unsupported top-level type: {other}"),
                });
            }
        }

        let bbox = compute_bbox(root, &records);

        Ok(Self { records, bbox })
    }

    /// Returns the bounding box of all records.
    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    /// Returns the number of records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if there are no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns all records, or at most `limit` records if specified.
    pub fn records(&self, limit: Option<usize>) -> Vec<&ShapeRecord> {
        match limit {
            Some(n) => self.records.iter().take(n).collect(),
            None => self.records.iter().collect(),
        }
    }

    /// Returns an iterator over all records.
    pub fn iter_records(&self) -> impl Iterator<Item = &ShapeRecord> {
        self.records.iter()
    }

    /// Returns the record at the given index, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<&ShapeRecord> {
        self.records.get(index)
    }

    /// Filters records by exact attribute match.
    pub fn filter_by_attribute(&self, field: &str, value: &AttributeValue) -> Vec<&ShapeRecord> {
        self.records
            .iter()
            .filter(|r| r.attributes.get(field) == Some(value))
            .collect()
    }

    /// Filters records where the attribute is one of the given values.
    pub fn filter_by_attribute_in(
        &self,
        field: &str,
        values: &[AttributeValue],
    ) -> Vec<&ShapeRecord> {
        self.records
            .iter()
            .filter(|r| r.attributes.get(field).is_some_and(|v| values.contains(v)))
            .collect()
    }

    /// Filters records where a Text attribute starts with the given prefix.
    pub fn filter_by_attribute_starts_with(&self, field: &str, prefix: &str) -> Vec<&ShapeRecord> {
        self.records
            .iter()
            .filter(|r| {
                r.attributes
                    .get(field)
                    .is_some_and(|v| v.starts_with(prefix))
            })
            .collect()
    }

    /// Filters records whose geometry bounding box intersects the given bbox.
    pub fn filter_by_bbox(&self, bbox: &BoundingBox) -> Vec<&ShapeRecord> {
        self.records
            .iter()
            .filter(|r| r.geometry.bbox().is_some_and(|gb| gb.intersects(bbox)))
            .collect()
    }

    /// Computes descriptive statistics for a numeric attribute field.
    pub fn describe(&self, field: &str) -> Result<FieldStats, ShapefileError> {
        // Check that the field exists in at least one record
        let field_exists = self
            .records
            .iter()
            .any(|r| r.attributes.contains_key(field));

        if !field_exists {
            return Err(ShapefileError::FieldNotFound(field.to_string()));
        }

        let mut values: Vec<f64> = self
            .records
            .iter()
            .filter_map(|r| r.attributes.get(field).and_then(|v| v.as_f64()))
            .collect();

        if values.is_empty() {
            return Err(ShapefileError::DescribeOnNonNumericField {
                field: field.to_string(),
                field_type: "non-numeric".to_string(),
            });
        }

        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let count = values.len();
        let min = values[0];
        let max = values[count - 1];
        let mean = values.iter().sum::<f64>() / count as f64;
        let median = if count % 2 == 0 {
            (values[count / 2 - 1] + values[count / 2]) / 2.0
        } else {
            values[count / 2]
        };

        Ok(FieldStats {
            count,
            min,
            max,
            mean,
            median,
        })
    }

    /// Serializes records back to a GeoJSON FeatureCollection string.
    /// Optionally limits the number of features.
    pub fn to_geojson(&self, limit: Option<usize>) -> Result<String, ShapefileError> {
        use serde_json::json;

        let recs = self.records(limit);
        let features: Vec<Value> = recs
            .into_iter()
            .map(|r| {
                let geometry = geometry_to_geojson_value(&r.geometry);
                let properties: serde_json::Map<String, Value> = r
                    .attributes
                    .iter()
                    .map(|(k, v)| {
                        let jv = match v {
                            AttributeValue::Text(s) => Value::String(s.clone()),
                            AttributeValue::Numeric(n) => {
                                json!(*n)
                            }
                            AttributeValue::Logical(b) => Value::Bool(*b),
                            AttributeValue::Date(d) => Value::String(d.clone()),
                            AttributeValue::Null => Value::Null,
                        };
                        (k.clone(), jv)
                    })
                    .collect();
                json!({
                    "type": "Feature",
                    "geometry": geometry,
                    "properties": properties,
                })
            })
            .collect();

        let fc = json!({
            "type": "FeatureCollection",
            "features": features,
        });

        Ok(serde_json::to_string_pretty(&fc)?)
    }
}

fn geometry_to_geojson_value(geom: &Geometry) -> Value {
    use serde_json::json;

    match geom {
        Geometry::Null => Value::Null,
        Geometry::Point(p) => json!({
            "type": "Point",
            "coordinates": [p.x, p.y],
        }),
        Geometry::Polyline(pl) => {
            if pl.parts.len() == 1 {
                let coords: Vec<[f64; 2]> = pl.parts[0].iter().map(|p| [p.x, p.y]).collect();
                json!({
                    "type": "LineString",
                    "coordinates": coords,
                })
            } else {
                let coords: Vec<Vec<[f64; 2]>> = pl
                    .parts
                    .iter()
                    .map(|part| part.iter().map(|p| [p.x, p.y]).collect())
                    .collect();
                json!({
                    "type": "MultiLineString",
                    "coordinates": coords,
                })
            }
        }
        Geometry::Polygon(pg) => {
            let coords: Vec<Vec<[f64; 2]>> = pg
                .rings
                .iter()
                .map(|ring| ring.points.iter().map(|p| [p.x, p.y]).collect())
                .collect();
            json!({
                "type": "Polygon",
                "coordinates": coords,
            })
        }
        Geometry::MultiPoint(mp) => {
            let coords: Vec<[f64; 2]> = mp.points.iter().map(|p| [p.x, p.y]).collect();
            json!({
                "type": "MultiPoint",
                "coordinates": coords,
            })
        }
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_point() {
        let coords = vec![json!(139.6917), json!(35.6895)];
        let p = parse_point(&coords).unwrap();
        assert!((p.x - 139.6917).abs() < 1e-10);
        assert!((p.y - 35.6895).abs() < 1e-10);
    }

    #[test]
    fn test_parse_linestring() {
        let geojson = json!({
            "type": "LineString",
            "coordinates": [[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]
        });
        let geoms = convert_geometry(&geojson).unwrap();
        assert_eq!(geoms.len(), 1);
        if let Geometry::Polyline(pl) = &geoms[0] {
            assert_eq!(pl.parts.len(), 1);
            assert_eq!(pl.parts[0].len(), 3);
        } else {
            panic!("expected Polyline");
        }
    }

    #[test]
    fn test_parse_multilinestring() {
        let geojson = json!({
            "type": "MultiLineString",
            "coordinates": [
                [[0.0, 0.0], [1.0, 1.0]],
                [[2.0, 2.0], [3.0, 3.0]]
            ]
        });
        let geoms = convert_geometry(&geojson).unwrap();
        assert_eq!(geoms.len(), 1);
        if let Geometry::Polyline(pl) = &geoms[0] {
            assert_eq!(pl.parts.len(), 2);
        } else {
            panic!("expected Polyline");
        }
    }

    #[test]
    fn test_parse_polygon() {
        let geojson = json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]],
                [[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0], [2.0, 2.0]]
            ]
        });
        let geoms = convert_geometry(&geojson).unwrap();
        assert_eq!(geoms.len(), 1);
        if let Geometry::Polygon(pg) = &geoms[0] {
            assert_eq!(pg.rings.len(), 2);
            assert_eq!(pg.exterior().points.len(), 5);
            assert_eq!(pg.holes().len(), 1);
        } else {
            panic!("expected Polygon");
        }
    }

    #[test]
    fn test_parse_multipolygon_expansion() {
        let geojson = json!({
            "type": "Feature",
            "geometry": {
                "type": "MultiPolygon",
                "coordinates": [
                    [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                    [[[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 2.0]]]
                ]
            },
            "properties": {"name": "test"}
        });
        let records = convert_feature(&geojson, 1).unwrap();
        assert_eq!(records.len(), 2);
        // Both records share the same attributes
        assert_eq!(
            records[0].attributes.get("name"),
            Some(&AttributeValue::Text("test".to_string()))
        );
        assert_eq!(
            records[1].attributes.get("name"),
            Some(&AttributeValue::Text("test".to_string()))
        );
    }

    #[test]
    fn test_parse_multipoint() {
        let geojson = json!({
            "type": "MultiPoint",
            "coordinates": [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]
        });
        let geoms = convert_geometry(&geojson).unwrap();
        assert_eq!(geoms.len(), 1);
        if let Geometry::MultiPoint(mp) = &geoms[0] {
            assert_eq!(mp.points.len(), 3);
        } else {
            panic!("expected MultiPoint");
        }
    }

    #[test]
    fn test_properties_string() {
        let props = json!({"name": "Tokyo"});
        let map = convert_properties(&props);
        assert_eq!(
            map.get("name"),
            Some(&AttributeValue::Text("Tokyo".to_string()))
        );
    }

    #[test]
    fn test_properties_number() {
        let props = json!({"pop": 1400});
        let map = convert_properties(&props);
        assert_eq!(map.get("pop"), Some(&AttributeValue::Numeric(1400.0)));
    }

    #[test]
    fn test_properties_boolean() {
        let props = json!({"active": true});
        let map = convert_properties(&props);
        assert_eq!(map.get("active"), Some(&AttributeValue::Logical(true)));
    }

    #[test]
    fn test_properties_null() {
        let props = json!({"val": null});
        let map = convert_properties(&props);
        assert_eq!(map.get("val"), Some(&AttributeValue::Null));
    }

    #[test]
    fn test_properties_nested_object() {
        let props = json!({"meta": {"a": 1, "b": 2}});
        let map = convert_properties(&props);
        if let Some(AttributeValue::Text(s)) = map.get("meta") {
            // Should be a JSON string
            assert!(s.contains("\"a\""));
            assert!(s.contains("\"b\""));
        } else {
            panic!("expected Text for nested object");
        }
    }

    #[test]
    fn test_empty_feature_collection() {
        let json_str = r#"{"type": "FeatureCollection", "features": []}"#;
        let reader = GeoJsonReader::from_str(json_str).unwrap();
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
    }

    #[test]
    fn test_single_feature_input() {
        let json_str = r#"{
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [1.0, 2.0]},
            "properties": {"name": "A"}
        }"#;
        let reader = GeoJsonReader::from_str(json_str).unwrap();
        assert_eq!(reader.len(), 1);
    }

    #[test]
    fn test_single_geometry_input() {
        let json_str = r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#;
        let reader = GeoJsonReader::from_str(json_str).unwrap();
        assert_eq!(reader.len(), 1);
        assert!(reader.get(0).unwrap().attributes.is_empty());
    }

    #[test]
    fn test_bbox_from_geojson_field() {
        let json_str = r#"{
            "type": "FeatureCollection",
            "bbox": [100.0, 0.0, 105.0, 1.0],
            "features": [
                {
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [102.0, 0.5]},
                    "properties": {}
                }
            ]
        }"#;
        let reader = GeoJsonReader::from_str(json_str).unwrap();
        let bb = reader.bbox();
        assert!((bb.x_min - 100.0).abs() < 1e-10);
        assert!((bb.y_min - 0.0).abs() < 1e-10);
        assert!((bb.x_max - 105.0).abs() < 1e-10);
        assert!((bb.y_max - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_bbox_computed_from_records() {
        let json_str = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [1.0, 2.0]},
                    "properties": {}
                },
                {
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [3.0, 4.0]},
                    "properties": {}
                }
            ]
        }"#;
        let reader = GeoJsonReader::from_str(json_str).unwrap();
        let bb = reader.bbox();
        assert!((bb.x_min - 1.0).abs() < 1e-10);
        assert!((bb.y_min - 2.0).abs() < 1e-10);
        assert!((bb.x_max - 3.0).abs() < 1e-10);
        assert!((bb.y_max - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_invalid_type_error() {
        let json_str = r#"{"type": "Unknown"}"#;
        let result = GeoJsonReader::from_str(json_str);
        assert!(result.is_err());
        if let Err(ShapefileError::InvalidGeoJson { reason }) = result {
            assert!(reason.contains("unsupported"));
        } else {
            panic!("expected InvalidGeoJson");
        }
    }

    #[test]
    fn test_geometry_collection_error() {
        let json_str = r#"{
            "type": "Feature",
            "geometry": {
                "type": "GeometryCollection",
                "geometries": []
            },
            "properties": {}
        }"#;
        let result = GeoJsonReader::from_str(json_str);
        assert!(result.is_err());
        if let Err(ShapefileError::InvalidGeoJson { reason }) = result {
            assert!(reason.contains("GeometryCollection"));
        } else {
            panic!("expected InvalidGeoJson");
        }
    }

    #[test]
    fn test_missing_coordinates_error() {
        let json_str = r#"{"type": "Point"}"#;
        let result = GeoJsonReader::from_str(json_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_point_insufficient_coords() {
        let json_str = r#"{"type": "Point", "coordinates": [1.0]}"#;
        let result = GeoJsonReader::from_str(json_str);
        assert!(result.is_err());
        if let Err(ShapefileError::InvalidGeoJson { reason }) = result {
            assert!(reason.contains("at least 2"));
        } else {
            panic!("expected InvalidGeoJson");
        }
    }
}
