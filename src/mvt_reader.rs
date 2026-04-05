//! MVT (Mapbox Vector Tile) reader that decodes protobuf-encoded vector tiles
//! into the library's shared model types.
//!
//! This module is only available when the `mvt` feature is enabled.

#![cfg(feature = "mvt")]

use std::collections::HashMap;
use std::path::Path;

use prost::Message;

use crate::error::ShapefileError;
use crate::models::attribute::{AttributeValue, FieldStats};
use crate::models::bbox::BoundingBox;
use crate::models::geometry::{Geometry, MultiPoint, Point, Polygon, Polyline, Ring};
use crate::models::record::ShapeRecord;

/// Generated protobuf types for the Mapbox Vector Tile specification v2.1.
pub mod vector_tile {
    include!(concat!(env!("OUT_DIR"), "/vector_tile.rs"));
}

// ---------------------------------------------------------------------------
// TileCoord
// ---------------------------------------------------------------------------

/// A Slippy Map tile coordinate at a given zoom level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    /// Zoom level.
    pub z: u32,
    /// Column (x) index.
    pub x: u32,
    /// Row (y) index.
    pub y: u32,
}

impl TileCoord {
    /// Creates a new tile coordinate.
    pub fn new(z: u32, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Converts this tile coordinate to a WGS 84 bounding box using the
    /// Slippy Map tile formulas.
    pub fn to_bbox(&self) -> BoundingBox {
        let n = (1u64 << self.z) as f64;
        let lon_min = self.x as f64 / n * 360.0 - 180.0;
        let lon_max = (self.x as f64 + 1.0) / n * 360.0 - 180.0;

        let lat_max = (std::f64::consts::PI * (1.0 - 2.0 * self.y as f64 / n))
            .sinh()
            .atan()
            .to_degrees();
        let lat_min = (std::f64::consts::PI * (1.0 - 2.0 * (self.y as f64 + 1.0) / n))
            .sinh()
            .atan()
            .to_degrees();

        BoundingBox {
            x_min: lon_min,
            y_min: lat_min,
            x_max: lon_max,
            y_max: lat_max,
        }
    }

    /// Converts a pixel coordinate within the tile to WGS 84 (lon, lat).
    ///
    /// `extent` is the tile extent (typically 4096).
    pub fn pixel_to_geo(&self, px: f64, py: f64, extent: u32) -> Point {
        let n = (1u64 << self.z) as f64;
        let ext = extent as f64;
        let lon = (self.x as f64 + px / ext) / n * 360.0 - 180.0;
        let lat = (std::f64::consts::PI * (1.0 - 2.0 * (self.y as f64 + py / ext) / n))
            .sinh()
            .atan()
            .to_degrees();
        Point { x: lon, y: lat }
    }
}

// ---------------------------------------------------------------------------
// LayerFilter
// ---------------------------------------------------------------------------

/// A filter for selecting specific layers and/or feature codes from an MVT.
#[derive(Debug, Clone, Default)]
pub struct LayerFilter {
    /// If set, only features from these layers are included.
    pub layer_names: Option<Vec<String>>,
    /// If set, only features whose `ftCode` attribute matches one of these values are included.
    pub ft_codes: Option<Vec<i64>>,
}

impl LayerFilter {
    /// Preset filter for road centerline feature codes (2701-2704).
    pub fn road_centerline() -> Self {
        Self {
            layer_names: None,
            ft_codes: Some(vec![2701, 2702, 2703, 2704]),
        }
    }

    /// Filter by a single layer name.
    pub fn layer(name: impl Into<String>) -> Self {
        Self {
            layer_names: Some(vec![name.into()]),
            ft_codes: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal decoding helpers
// ---------------------------------------------------------------------------

/// ZigZag decoding: maps unsigned to signed.
fn decode_zigzag(n: u32) -> i32 {
    ((n >> 1) as i32) ^ -((n & 1) as i32)
}

/// Encode a signed integer using ZigZag encoding (for tests).
#[cfg(test)]
fn encode_zigzag(n: i32) -> u32 {
    ((n << 1) ^ (n >> 31)) as u32
}

/// Encode an MVT command word (for tests).
#[cfg(test)]
fn encode_command(id: u32, count: u32) -> u32 {
    (id & 0x7) | (count << 3)
}

/// Decode MVT geometry commands into `Geometry`.
///
/// MVT command IDs:
/// - 1 = MoveTo
/// - 2 = LineTo
/// - 7 = ClosePath
fn decode_geometry(
    feature: &vector_tile::tile::Feature,
    tile_coord: &TileCoord,
    extent: u32,
) -> Result<Geometry, ShapefileError> {
    let geom_type = feature.r#type();

    let cmds = &feature.geometry;
    let mut i = 0;
    let mut cx: i32 = 0;
    let mut cy: i32 = 0;

    // Collect decoded pixel-coordinate sequences.
    // For POINT: each MoveTo produces a separate point.
    // For LINESTRING: each MoveTo starts a new part.
    // For POLYGON: rings are delimited by ClosePath.

    match geom_type {
        vector_tile::tile::GeomType::Point => {
            let mut points = Vec::new();
            while i < cmds.len() {
                let cmd_int = cmds[i];
                let cmd_id = cmd_int & 0x7;
                let cmd_count = cmd_int >> 3;
                i += 1;

                if cmd_id != 1 {
                    return Err(ShapefileError::InvalidMvt {
                        reason: format!("expected MoveTo in POINT, got cmd_id={cmd_id}"),
                    });
                }

                for _ in 0..cmd_count {
                    if i + 1 >= cmds.len() {
                        return Err(ShapefileError::InvalidMvt {
                            reason: "truncated POINT geometry".to_string(),
                        });
                    }
                    cx += decode_zigzag(cmds[i]);
                    cy += decode_zigzag(cmds[i + 1]);
                    i += 2;
                    points.push(tile_coord.pixel_to_geo(cx as f64, cy as f64, extent));
                }
            }

            if points.len() == 1 {
                Ok(Geometry::Point(points.remove(0)))
            } else {
                Ok(Geometry::MultiPoint(MultiPoint { points }))
            }
        }

        vector_tile::tile::GeomType::Linestring => {
            let mut parts: Vec<Vec<Point>> = Vec::new();
            let mut current_part: Vec<Point> = Vec::new();

            while i < cmds.len() {
                let cmd_int = cmds[i];
                let cmd_id = cmd_int & 0x7;
                let cmd_count = cmd_int >> 3;
                i += 1;

                match cmd_id {
                    1 => {
                        // MoveTo: start new part
                        if !current_part.is_empty() {
                            parts.push(std::mem::take(&mut current_part));
                        }
                        for _ in 0..cmd_count {
                            if i + 1 >= cmds.len() {
                                return Err(ShapefileError::InvalidMvt {
                                    reason: "truncated LINESTRING geometry".to_string(),
                                });
                            }
                            cx += decode_zigzag(cmds[i]);
                            cy += decode_zigzag(cmds[i + 1]);
                            i += 2;
                            current_part
                                .push(tile_coord.pixel_to_geo(cx as f64, cy as f64, extent));
                        }
                    }
                    2 => {
                        // LineTo
                        for _ in 0..cmd_count {
                            if i + 1 >= cmds.len() {
                                return Err(ShapefileError::InvalidMvt {
                                    reason: "truncated LINESTRING geometry".to_string(),
                                });
                            }
                            cx += decode_zigzag(cmds[i]);
                            cy += decode_zigzag(cmds[i + 1]);
                            i += 2;
                            current_part
                                .push(tile_coord.pixel_to_geo(cx as f64, cy as f64, extent));
                        }
                    }
                    _ => {
                        return Err(ShapefileError::InvalidMvt {
                            reason: format!("unexpected command {cmd_id} in LINESTRING geometry"),
                        });
                    }
                }
            }

            if !current_part.is_empty() {
                parts.push(current_part);
            }

            Ok(Geometry::Polyline(Polyline { parts }))
        }

        vector_tile::tile::GeomType::Polygon => {
            let mut rings: Vec<Ring> = Vec::new();
            let mut current_ring: Vec<Point> = Vec::new();
            // Track pixel coords for winding-order determination (Shoelace on pixel space).
            let mut current_ring_px: Vec<(f64, f64)> = Vec::new();
            let mut polygons: Vec<Polygon> = Vec::new();

            while i < cmds.len() {
                let cmd_int = cmds[i];
                let cmd_id = cmd_int & 0x7;
                let cmd_count = cmd_int >> 3;
                i += 1;

                match cmd_id {
                    1 => {
                        // MoveTo
                        for _ in 0..cmd_count {
                            if i + 1 >= cmds.len() {
                                return Err(ShapefileError::InvalidMvt {
                                    reason: "truncated POLYGON geometry".to_string(),
                                });
                            }
                            cx += decode_zigzag(cmds[i]);
                            cy += decode_zigzag(cmds[i + 1]);
                            i += 2;
                            current_ring
                                .push(tile_coord.pixel_to_geo(cx as f64, cy as f64, extent));
                            current_ring_px.push((cx as f64, cy as f64));
                        }
                    }
                    2 => {
                        // LineTo
                        for _ in 0..cmd_count {
                            if i + 1 >= cmds.len() {
                                return Err(ShapefileError::InvalidMvt {
                                    reason: "truncated POLYGON geometry".to_string(),
                                });
                            }
                            cx += decode_zigzag(cmds[i]);
                            cy += decode_zigzag(cmds[i + 1]);
                            i += 2;
                            current_ring
                                .push(tile_coord.pixel_to_geo(cx as f64, cy as f64, extent));
                            current_ring_px.push((cx as f64, cy as f64));
                        }
                    }
                    7 => {
                        // ClosePath: close the ring (repeat first point)
                        if let Some(first) = current_ring.first().cloned() {
                            current_ring.push(first);
                        }
                        if let Some(&first_px) = current_ring_px.first() {
                            current_ring_px.push(first_px);
                        }

                        // Determine winding order from pixel coords.
                        // Positive signed area (in pixel space with Y-down) = clockwise = exterior.
                        let signed_area = shoelace_signed(&current_ring_px);

                        let ring = Ring {
                            points: std::mem::take(&mut current_ring),
                        };
                        current_ring_px.clear();

                        if signed_area > 0.0 {
                            // Exterior ring: push previous polygon if any, start new
                            if !rings.is_empty() {
                                polygons.push(Polygon {
                                    rings: std::mem::take(&mut rings),
                                });
                            }
                            rings.push(ring);
                        } else {
                            // Interior (hole)
                            rings.push(ring);
                        }
                    }
                    _ => {
                        return Err(ShapefileError::InvalidMvt {
                            reason: format!("unexpected command {cmd_id} in POLYGON geometry"),
                        });
                    }
                }
            }

            if !rings.is_empty() {
                polygons.push(Polygon { rings });
            }

            if polygons.len() == 1 {
                Ok(Geometry::Polygon(polygons.remove(0)))
            } else if polygons.is_empty() {
                Ok(Geometry::Null)
            } else {
                // Multiple polygons: just take the first for simplicity
                // (MVT features typically produce a single polygon).
                // We merge all rings into one Polygon with the first as
                // exterior and the rest as additional rings.
                let mut all_rings = Vec::new();
                for p in polygons {
                    all_rings.extend(p.rings);
                }
                Ok(Geometry::Polygon(Polygon { rings: all_rings }))
            }
        }

        vector_tile::tile::GeomType::Unknown => Ok(Geometry::Null),
    }
}

/// Signed area using the Shoelace formula on pixel coordinates.
/// In MVT pixel space (Y increases downward), a positive result means clockwise
/// winding, which corresponds to an exterior ring.
fn shoelace_signed(coords: &[(f64, f64)]) -> f64 {
    let n = coords.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i].0 * coords[j].1 - coords[j].0 * coords[i].1;
    }
    area / 2.0
}

/// Decode attribute tags from an MVT feature using the layer's keys/values tables.
fn decode_attributes(
    feature: &vector_tile::tile::Feature,
    layer: &vector_tile::tile::Layer,
) -> HashMap<String, AttributeValue> {
    let mut attrs = HashMap::new();
    let tags = &feature.tags;

    let mut t = 0;
    while t + 1 < tags.len() {
        let key_idx = tags[t] as usize;
        let val_idx = tags[t + 1] as usize;
        t += 2;

        if key_idx < layer.keys.len() && val_idx < layer.values.len() {
            let key = layer.keys[key_idx].clone();
            let val = mvt_value_to_attribute(&layer.values[val_idx]);
            attrs.insert(key, val);
        }
    }

    attrs
}

/// Convert a protobuf `Value` to an `AttributeValue`.
fn mvt_value_to_attribute(val: &vector_tile::tile::Value) -> AttributeValue {
    if let Some(ref s) = val.string_value {
        return AttributeValue::Text(s.clone());
    }
    if let Some(v) = val.double_value {
        return AttributeValue::Numeric(v);
    }
    if let Some(v) = val.float_value {
        return AttributeValue::Numeric(v as f64);
    }
    if let Some(v) = val.int_value {
        return AttributeValue::Numeric(v as f64);
    }
    if let Some(v) = val.uint_value {
        return AttributeValue::Numeric(v as f64);
    }
    if let Some(v) = val.sint_value {
        return AttributeValue::Numeric(v as f64);
    }
    if let Some(v) = val.bool_value {
        return AttributeValue::Logical(v);
    }
    AttributeValue::Null
}

// ---------------------------------------------------------------------------
// MvtReader
// ---------------------------------------------------------------------------

/// A reader for MVT (Mapbox Vector Tile) files that provides the same analysis
/// API as [`GeoJsonReader`](crate::geojson_reader::GeoJsonReader).
///
/// All records are decoded into memory at construction time, so accessor methods
/// return borrowed references.
pub struct MvtReader {
    /// The tile coordinate used during decoding.
    tile: TileCoord,
    /// Decoded shape records.
    records: Vec<ShapeRecord>,
    /// Bounding box derived from the tile coordinate.
    bbox: BoundingBox,
    /// Names of the layers found in the tile.
    layer_names_list: Vec<String>,
}

impl MvtReader {
    /// Opens an MVT file at the given path and decodes it using the provided tile coordinate.
    pub fn open(path: impl AsRef<Path>, tile: TileCoord) -> Result<Self, ShapefileError> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes, tile)
    }

    /// Decodes MVT data from raw bytes using the provided tile coordinate.
    pub fn from_bytes(bytes: &[u8], tile: TileCoord) -> Result<Self, ShapefileError> {
        let proto_tile = vector_tile::Tile::decode(bytes)?;
        let bbox = tile.to_bbox();

        let mut records = Vec::new();
        let mut layer_names_list = Vec::new();
        let mut record_number: u32 = 1;

        for layer in &proto_tile.layers {
            layer_names_list.push(layer.name.clone());
            let extent = layer.extent.unwrap_or(4096);

            for feature in &layer.features {
                let geometry = decode_geometry(feature, &tile, extent)?;
                let mut attributes = decode_attributes(feature, layer);

                // Inject the layer name as a special attribute.
                attributes.insert(
                    "_layer".to_string(),
                    AttributeValue::Text(layer.name.clone()),
                );

                records.push(ShapeRecord {
                    record_number,
                    geometry,
                    attributes,
                });
                record_number += 1;
            }
        }

        Ok(Self {
            tile,
            records,
            bbox,
            layer_names_list,
        })
    }

    /// Returns the tile coordinate used to decode this tile.
    pub fn tile(&self) -> &TileCoord {
        &self.tile
    }

    /// Returns the bounding box of the tile (derived from the tile coordinate).
    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    /// Returns the number of decoded records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if there are no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns the names of all layers found in the tile.
    pub fn layer_names(&self) -> &[String] {
        &self.layer_names_list
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

    /// Filters records by layer name (checks the `_layer` attribute).
    pub fn filter_by_layer(&self, layer: &str) -> Vec<&ShapeRecord> {
        let target = AttributeValue::Text(layer.to_string());
        self.records
            .iter()
            .filter(|r| r.attributes.get("_layer") == Some(&target))
            .collect()
    }

    /// Filters records using a [`LayerFilter`], checking both layer names and feature codes.
    pub fn filter_by_layer_filter(&self, filter: &LayerFilter) -> Vec<&ShapeRecord> {
        self.records
            .iter()
            .filter(|r| {
                let layer_ok = match &filter.layer_names {
                    Some(names) => {
                        if let Some(AttributeValue::Text(layer_name)) = r.attributes.get("_layer") {
                            names.iter().any(|n| n == layer_name)
                        } else {
                            false
                        }
                    }
                    None => true,
                };

                let ft_ok = match &filter.ft_codes {
                    Some(codes) => {
                        if let Some(AttributeValue::Numeric(ft)) = r.attributes.get("ftCode") {
                            codes.contains(&(*ft as i64))
                        } else {
                            false
                        }
                    }
                    None => true,
                };

                layer_ok && ft_ok
            })
            .collect()
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

    /// Serializes records to a GeoJSON FeatureCollection string.
    ///
    /// Requires both the `mvt` and `geojson` features to be enabled.
    #[cfg(feature = "geojson")]
    pub fn to_geojson(&self, limit: Option<usize>) -> Result<String, ShapefileError> {
        use serde_json::{json, Value};

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
                            AttributeValue::Numeric(n) => json!(*n),
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

#[cfg(feature = "geojson")]
fn geometry_to_geojson_value(geom: &Geometry) -> serde_json::Value {
    use serde_json::json;

    match geom {
        Geometry::Null => serde_json::Value::Null,
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
        _ => serde_json::Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_coord_bbox_z14() {
        // z=14, x=14552, y=6451 should roughly cover a small area in Tokyo
        let tc = TileCoord::new(14, 14552, 6451);
        let bb = tc.to_bbox();
        assert!(bb.x_min < bb.x_max);
        assert!(bb.y_min < bb.y_max);
        // Lon should be in the 139-140 range for Tokyo
        assert!(bb.x_min > 139.0 && bb.x_min < 140.0);
    }

    #[test]
    fn test_pixel_to_geo_origin() {
        let tc = TileCoord::new(14, 14552, 6451);
        let bb = tc.to_bbox();
        let p = tc.pixel_to_geo(0.0, 0.0, 4096);
        // pixel (0,0) should be top-left corner: (lon_min, lat_max)
        assert!((p.x - bb.x_min).abs() < 1e-8);
        assert!((p.y - bb.y_max).abs() < 1e-8);
    }

    #[test]
    fn test_pixel_to_geo_center() {
        let tc = TileCoord::new(14, 14552, 6451);
        let bb = tc.to_bbox();
        let p = tc.pixel_to_geo(2048.0, 2048.0, 4096);
        let mid_lon = (bb.x_min + bb.x_max) / 2.0;
        // Should be near the center
        assert!((p.x - mid_lon).abs() < 0.001);
    }

    #[test]
    fn test_decode_zigzag_positive() {
        // 0 -> 0, 2 -> 1, 4 -> 2, ...
        assert_eq!(decode_zigzag(0), 0);
        assert_eq!(decode_zigzag(2), 1);
        assert_eq!(decode_zigzag(4), 2);
        assert_eq!(decode_zigzag(100), 50);
    }

    #[test]
    fn test_decode_zigzag_negative() {
        // 1 -> -1, 3 -> -2, 5 -> -3, ...
        assert_eq!(decode_zigzag(1), -1);
        assert_eq!(decode_zigzag(3), -2);
        assert_eq!(decode_zigzag(5), -3);
    }

    #[test]
    fn test_decode_moveto_command() {
        let tc = TileCoord::new(0, 0, 0);
        let feature = vector_tile::tile::Feature {
            id: None,
            tags: vec![],
            r#type: Some(1), // POINT
            geometry: vec![
                encode_command(1, 1), // MoveTo x1
                encode_zigzag(100),
                encode_zigzag(200),
            ],
        };
        let geom = decode_geometry(&feature, &tc, 4096).unwrap();
        match geom {
            Geometry::Point(p) => {
                assert!(p.x.is_finite());
                assert!(p.y.is_finite());
            }
            _ => panic!("expected Point"),
        }
    }

    #[test]
    fn test_decode_lineto_command() {
        let tc = TileCoord::new(0, 0, 0);
        let feature = vector_tile::tile::Feature {
            id: None,
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
        };
        let geom = decode_geometry(&feature, &tc, 4096).unwrap();
        match geom {
            Geometry::Polyline(pl) => {
                assert_eq!(pl.parts.len(), 1);
                assert_eq!(pl.parts[0].len(), 3); // 1 moveto + 2 lineto
            }
            _ => panic!("expected Polyline"),
        }
    }

    #[test]
    fn test_decode_closepath_command() {
        let tc = TileCoord::new(0, 0, 0);
        // Clockwise triangle (exterior ring in pixel-Y-down space)
        let feature = vector_tile::tile::Feature {
            id: None,
            tags: vec![],
            r#type: Some(3), // POLYGON
            geometry: vec![
                encode_command(1, 1), // MoveTo
                encode_zigzag(0),
                encode_zigzag(0),
                encode_command(2, 2), // LineTo x2
                encode_zigzag(100),
                encode_zigzag(0),
                encode_zigzag(0),
                encode_zigzag(100),
                encode_command(7, 1), // ClosePath
            ],
        };
        let geom = decode_geometry(&feature, &tc, 4096).unwrap();
        match geom {
            Geometry::Polygon(pg) => {
                // Should have 1 ring (exterior)
                assert!(!pg.rings.is_empty());
                // Ring should be closed (first == last)
                let ring = &pg.rings[0];
                assert_eq!(ring.points.first(), ring.points.last());
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn test_polygon_winding_exterior() {
        // Clockwise in pixel space (Y-down): positive signed area
        let coords = vec![
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (0.0, 100.0),
            (0.0, 0.0),
        ];
        let area = shoelace_signed(&coords);
        // In Y-down pixel space, this order is clockwise -> positive
        assert!(area > 0.0, "expected positive for CW, got {area}");
    }

    #[test]
    fn test_polygon_winding_interior() {
        // Counter-clockwise in pixel space: negative signed area
        let coords = vec![
            (0.0, 0.0),
            (0.0, 100.0),
            (100.0, 100.0),
            (100.0, 0.0),
            (0.0, 0.0),
        ];
        let area = shoelace_signed(&coords);
        assert!(area < 0.0, "expected negative for CCW, got {area}");
    }

    #[test]
    fn test_mvt_value_string() {
        let val = vector_tile::tile::Value {
            string_value: Some("hello".to_string()),
            ..Default::default()
        };
        assert_eq!(
            mvt_value_to_attribute(&val),
            AttributeValue::Text("hello".to_string())
        );
    }

    #[test]
    fn test_mvt_value_int() {
        let val = vector_tile::tile::Value {
            int_value: Some(42),
            ..Default::default()
        };
        assert_eq!(mvt_value_to_attribute(&val), AttributeValue::Numeric(42.0));
    }

    #[test]
    fn test_mvt_value_bool() {
        let val = vector_tile::tile::Value {
            bool_value: Some(true),
            ..Default::default()
        };
        assert_eq!(mvt_value_to_attribute(&val), AttributeValue::Logical(true));
    }

    #[test]
    fn test_layer_name_injected() {
        let layer = vector_tile::tile::Layer {
            version: 2,
            name: "roads".to_string(),
            features: vec![vector_tile::tile::Feature {
                id: Some(1),
                tags: vec![],
                r#type: Some(1), // POINT
                geometry: vec![encode_command(1, 1), encode_zigzag(10), encode_zigzag(20)],
            }],
            keys: vec![],
            values: vec![],
            extent: Some(4096),
        };
        let tile = vector_tile::Tile {
            layers: vec![layer],
        };
        let bytes = tile.encode_to_vec();
        let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();
        let rec = reader.get(0).unwrap();
        assert_eq!(
            rec.attributes.get("_layer"),
            Some(&AttributeValue::Text("roads".to_string()))
        );
    }

    #[test]
    fn test_from_bytes_minimal() {
        let layer = vector_tile::tile::Layer {
            version: 2,
            name: "test".to_string(),
            features: vec![vector_tile::tile::Feature {
                id: Some(1),
                tags: vec![0, 0],
                r#type: Some(1), // POINT
                geometry: vec![encode_command(1, 1), encode_zigzag(100), encode_zigzag(200)],
            }],
            keys: vec!["name".to_string()],
            values: vec![vector_tile::tile::Value {
                string_value: Some("test_point".to_string()),
                ..Default::default()
            }],
            extent: Some(4096),
        };
        let tile = vector_tile::Tile {
            layers: vec![layer],
        };
        let bytes = tile.encode_to_vec();
        let reader = MvtReader::from_bytes(&bytes, TileCoord::new(14, 14552, 6451)).unwrap();

        assert_eq!(reader.len(), 1);
        let rec = reader.get(0).unwrap();
        assert_eq!(
            rec.attributes.get("name"),
            Some(&AttributeValue::Text("test_point".to_string()))
        );
    }

    #[test]
    fn test_from_bytes_empty_tile() {
        let tile = vector_tile::Tile { layers: vec![] };
        let bytes = tile.encode_to_vec();
        let reader = MvtReader::from_bytes(&bytes, TileCoord::new(0, 0, 0)).unwrap();
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
    }

    #[test]
    fn test_from_bytes_invalid_proto() {
        let result = MvtReader::from_bytes(&[0xFF, 0xFE, 0xFD], TileCoord::new(0, 0, 0));
        assert!(result.is_err());
    }
}
