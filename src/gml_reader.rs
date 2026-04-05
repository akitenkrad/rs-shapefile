//! GML/JPGIS2.1 reader that converts GML XML into the library's model types.
//!
//! This module is only available when the `gml` feature is enabled.
//! It supports the GML profile used by the Japanese National Land Numerical
//! Information download service (JPGIS2.1 / KSJ-style datasets).

#![cfg(feature = "gml")]

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::ShapefileError;
use crate::models::attribute::{AttributeValue, FieldStats};
use crate::models::bbox::BoundingBox;
use crate::models::geometry::{Geometry, Point, Polygon, Polyline, Ring};
use crate::models::record::ShapeRecord;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// GML namespace prefix.
const NS_GML: &str = "gml";

/// KSJ namespace prefix (used by JPGIS2.1 datasets).
#[allow(dead_code)]
const NS_KSJ: &str = "ksj";

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extracts the local name from a potentially namespace-prefixed tag.
///
/// For example, `b"gml:Point"` returns `b"Point"`, while `b"Road"` returns `b"Road"`.
fn local_name(tag: &[u8]) -> &[u8] {
    match tag.iter().position(|&b| b == b':') {
        Some(pos) => &tag[pos + 1..],
        None => tag,
    }
}

/// Returns the namespace prefix from a potentially prefixed tag.
///
/// For example, `b"gml:Point"` returns `Some(b"gml")`, while `b"Road"` returns `None`.
fn namespace_prefix(tag: &[u8]) -> Option<&[u8]> {
    match tag.iter().position(|&b| b == b':') {
        Some(pos) => Some(&tag[..pos]),
        None => None,
    }
}

/// Parses a `gml:posList` text: space-separated `lat lon lat lon ...` pairs.
///
/// GML uses latitude-first ordering. This function converts each pair to a
/// `Point { x: lon, y: lat }`.
fn parse_pos_list(text: &str) -> Result<Vec<Point>, ShapefileError> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() % 2 != 0 {
        return Err(ShapefileError::InvalidGml {
            reason: format!("posList has odd number of coordinates ({})", tokens.len()),
        });
    }
    let mut points = Vec::with_capacity(tokens.len() / 2);
    for chunk in tokens.chunks(2) {
        let lat: f64 = chunk[0].parse().map_err(|_| ShapefileError::InvalidGml {
            reason: format!("invalid float in posList: '{}'", chunk[0]),
        })?;
        let lon: f64 = chunk[1].parse().map_err(|_| ShapefileError::InvalidGml {
            reason: format!("invalid float in posList: '{}'", chunk[1]),
        })?;
        points.push(Point { x: lon, y: lat });
    }
    Ok(points)
}

/// Parses a `gml:pos` text: a single `lat lon` coordinate pair.
fn parse_pos(text: &str) -> Result<Point, ShapefileError> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() < 2 {
        return Err(ShapefileError::InvalidGml {
            reason: format!("pos requires 2 coordinates, got {}", tokens.len()),
        });
    }
    let lat: f64 = tokens[0].parse().map_err(|_| ShapefileError::InvalidGml {
        reason: format!("invalid float in pos: '{}'", tokens[0]),
    })?;
    let lon: f64 = tokens[1].parse().map_err(|_| ShapefileError::InvalidGml {
        reason: format!("invalid float in pos: '{}'", tokens[1]),
    })?;
    Ok(Point { x: lon, y: lat })
}

/// Parses `gml:coordinates` text: comma-separated pairs delimited by whitespace or semicolons.
///
/// Format: `lat,lon lat,lon ...` or `lat,lon;lat,lon;...`
fn parse_coordinates(text: &str) -> Result<Vec<Point>, ShapefileError> {
    let normalized = text.replace(';', " ");
    let tuples: Vec<&str> = normalized.split_whitespace().collect();
    let mut points = Vec::with_capacity(tuples.len());
    for tuple in tuples {
        let parts: Vec<&str> = tuple.split(',').collect();
        if parts.len() < 2 {
            return Err(ShapefileError::InvalidGml {
                reason: format!("coordinates tuple needs at least 2 values: '{tuple}'"),
            });
        }
        let lat: f64 = parts[0].parse().map_err(|_| ShapefileError::InvalidGml {
            reason: format!("invalid float in coordinates: '{}'", parts[0]),
        })?;
        let lon: f64 = parts[1].parse().map_err(|_| ShapefileError::InvalidGml {
            reason: format!("invalid float in coordinates: '{}'", parts[1]),
        })?;
        points.push(Point { x: lon, y: lat });
    }
    Ok(points)
}

/// Converts an attribute text value: try f64 first, otherwise Text; empty means Null.
fn parse_attribute_value(text: &str) -> AttributeValue {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return AttributeValue::Null;
    }
    match trimmed.parse::<f64>() {
        Ok(n) => AttributeValue::Numeric(n),
        Err(_) => AttributeValue::Text(trimmed.to_string()),
    }
}

/// Computes a bounding box from a list of records.
fn compute_bbox_from_records(records: &[ShapeRecord]) -> BoundingBox {
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
// Parser state machine
// ---------------------------------------------------------------------------

/// Tracks what kind of geometry element we are currently inside.
#[derive(Debug, Clone, PartialEq)]
enum GeomState {
    None,
    Point,
    LineString,
    Curve,
    MultiCurve,
    Polygon,
    Surface,
    MultiSurface,
}

/// Internal state for the SAX-style parser.
struct ParseState {
    /// Stack of element local names (for tracking depth).
    path: Vec<String>,
    /// Whether we are inside a `gml:featureMember`.
    in_feature_member: bool,
    /// Whether we are inside the feature element (direct child of featureMember).
    in_feature: bool,
    /// Feature type name (e.g. "Road").
    feature_type: Option<String>,
    /// `gml:id` of the current feature.
    feature_id: Option<String>,
    /// Current geometry state.
    geom_state: GeomState,
    /// Whether we are inside `gml:exterior`.
    in_exterior: bool,
    /// Whether we are inside `gml:interior`.
    in_interior: bool,
    /// Whether we are inside a `gml:curveMember`.
    in_curve_member: bool,
    /// Whether we are inside a `gml:surfaceMember`.
    in_surface_member: bool,
    /// Whether we are inside `gml:boundedBy`.
    in_bounded_by: bool,
    /// Whether we are inside `gml:Envelope`.
    in_envelope: bool,
    /// Whether the current text capture is for a `gml:lowerCorner`.
    in_lower_corner: bool,
    /// Whether the current text capture is for a `gml:upperCorner`.
    in_upper_corner: bool,
    /// Accumulated text for the current element.
    text_buf: String,
    /// Current element's local name (for attribute capture).
    current_element: Option<String>,
    /// Whether the current element is a GML element (not an attribute).
    current_is_gml: bool,
    /// Depth at which the feature element starts.
    feature_depth: usize,
    /// Accumulated attributes for the current feature.
    attributes: HashMap<String, AttributeValue>,
    /// Accumulated points for the current geometry.
    points: Vec<Point>,
    /// Exterior ring points for Polygon.
    exterior_ring: Option<Vec<Point>>,
    /// Interior ring points for Polygon.
    interior_rings: Vec<Vec<Point>>,
    /// Parts for MultiCurve.
    line_parts: Vec<Vec<Point>>,
    /// Polygons for MultiSurface expansion.
    surface_polygons: Vec<Polygon>,
    /// srsName extracted from the first geometry element.
    srs_name: Option<String>,
    /// Whether srsName has been captured yet.
    srs_captured: bool,
    /// Envelope lower corner.
    envelope_lower: Option<Point>,
    /// Envelope upper corner.
    envelope_upper: Option<Point>,
    /// Completed records.
    records: Vec<ShapeRecord>,
    /// Next record number.
    next_record_number: u32,
}

impl ParseState {
    fn new() -> Self {
        Self {
            path: Vec::new(),
            in_feature_member: false,
            in_feature: false,
            feature_type: None,
            feature_id: None,
            geom_state: GeomState::None,
            in_exterior: false,
            in_interior: false,
            in_curve_member: false,
            in_surface_member: false,
            in_bounded_by: false,
            in_envelope: false,
            in_lower_corner: false,
            in_upper_corner: false,
            text_buf: String::new(),
            current_element: None,
            current_is_gml: false,
            feature_depth: 0,
            attributes: HashMap::new(),
            points: Vec::new(),
            exterior_ring: None,
            interior_rings: Vec::new(),
            line_parts: Vec::new(),
            surface_polygons: Vec::new(),
            srs_name: None,
            srs_captured: false,
            envelope_lower: None,
            envelope_upper: None,
            records: Vec::new(),
            next_record_number: 1,
        }
    }

    fn reset_feature(&mut self) {
        self.in_feature = false;
        self.feature_type = None;
        self.feature_id = None;
        self.geom_state = GeomState::None;
        self.in_exterior = false;
        self.in_interior = false;
        self.in_curve_member = false;
        self.in_surface_member = false;
        self.text_buf.clear();
        self.current_element = None;
        self.current_is_gml = false;
        self.feature_depth = 0;
        self.attributes.clear();
        self.points.clear();
        self.exterior_ring = None;
        self.interior_rings.clear();
        self.line_parts.clear();
        self.surface_polygons.clear();
    }
}

// ---------------------------------------------------------------------------
// Core parse logic
// ---------------------------------------------------------------------------

fn parse_gml<R: BufRead>(
    reader: R,
) -> Result<(Vec<ShapeRecord>, BoundingBox, Option<String>), ShapefileError> {
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(true);

    let mut state = ParseState::new();
    let mut buf = Vec::new();

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = e.name();
                let tag_bytes = tag_name.as_ref();
                let local = local_name(tag_bytes);
                let prefix = namespace_prefix(tag_bytes);
                let local_str = String::from_utf8_lossy(local).to_string();
                let is_gml = prefix.is_some_and(|p| p == NS_GML.as_bytes());

                state.path.push(local_str.clone());
                state.text_buf.clear();

                if is_gml && local == b"boundedBy" {
                    state.in_bounded_by = true;
                } else if is_gml && local == b"Envelope" && state.in_bounded_by {
                    state.in_envelope = true;
                    // Extract srsName from Envelope if not yet captured
                    if !state.srs_captured {
                        for attr in e.attributes().flatten() {
                            if local_name(attr.key.as_ref()) == b"srsName" {
                                state.srs_name =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                                state.srs_captured = true;
                            }
                        }
                    }
                } else if is_gml && local == b"lowerCorner" && state.in_envelope {
                    state.in_lower_corner = true;
                } else if is_gml && local == b"upperCorner" && state.in_envelope {
                    state.in_upper_corner = true;
                } else if is_gml && (local == b"featureMember" || local == b"featureMembers") {
                    state.in_feature_member = true;
                } else if state.in_feature_member && !state.in_feature && !is_gml {
                    // This is the feature element itself
                    state.in_feature = true;
                    state.feature_depth = state.path.len();
                    state.feature_type = Some(local_str.clone());
                    // Extract gml:id
                    for attr in e.attributes().flatten() {
                        let attr_local = local_name(attr.key.as_ref());
                        if attr_local == b"id" {
                            state.feature_id =
                                Some(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                } else if state.in_feature && is_gml {
                    // GML geometry elements
                    match local {
                        b"Point" => {
                            state.geom_state = GeomState::Point;
                            extract_srs_name(e, &mut state);
                        }
                        b"LineString" => {
                            if state.geom_state != GeomState::MultiCurve {
                                state.geom_state = GeomState::LineString;
                            }
                            extract_srs_name(e, &mut state);
                        }
                        b"Curve" => {
                            if state.geom_state != GeomState::MultiCurve {
                                state.geom_state = GeomState::Curve;
                            }
                            extract_srs_name(e, &mut state);
                        }
                        b"MultiCurve" => {
                            state.geom_state = GeomState::MultiCurve;
                            extract_srs_name(e, &mut state);
                        }
                        b"Polygon" => {
                            if state.geom_state != GeomState::MultiSurface {
                                state.geom_state = GeomState::Polygon;
                            }
                            extract_srs_name(e, &mut state);
                        }
                        b"Surface" => {
                            if state.geom_state != GeomState::MultiSurface {
                                state.geom_state = GeomState::Surface;
                            }
                            extract_srs_name(e, &mut state);
                        }
                        b"MultiSurface" => {
                            state.geom_state = GeomState::MultiSurface;
                            extract_srs_name(e, &mut state);
                        }
                        b"exterior" => state.in_exterior = true,
                        b"interior" => state.in_interior = true,
                        b"curveMember" => state.in_curve_member = true,
                        b"surfaceMember" => state.in_surface_member = true,
                        b"GeometryCollection" => {
                            return Err(ShapefileError::InvalidGml {
                                reason: "GeometryCollection is not supported".to_string(),
                            });
                        }
                        _ => {}
                    }
                }

                // Track current element for attribute capture
                if state.in_feature {
                    state.current_element = Some(local_str);
                    state.current_is_gml = is_gml;
                }
            }
            Ok(Event::Empty(ref e)) => {
                // Self-closing elements -- handle the same as Start + immediate End
                let tag_name = e.name();
                let tag_bytes = tag_name.as_ref();
                let local = local_name(tag_bytes);
                let prefix = namespace_prefix(tag_bytes);
                let is_gml = prefix.is_some_and(|p| p == NS_GML.as_bytes());

                if is_gml && (local == b"featureMember" || local == b"featureMembers") {
                    // empty featureMember, skip
                } else if state.in_feature && is_gml && local == b"GeometryCollection" {
                    return Err(ShapefileError::InvalidGml {
                        reason: "GeometryCollection is not supported".to_string(),
                    });
                } else if state.in_feature && !is_gml && state.geom_state == GeomState::None {
                    // Self-closing attribute element -- treat as empty/null
                    let local_str = String::from_utf8_lossy(local).to_string();
                    state.attributes.insert(local_str, AttributeValue::Null);
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e
                    .unescape()
                    .map_err(|err| ShapefileError::GmlXmlError(err.to_string()))?;
                state.text_buf.push_str(&text);
            }
            Ok(Event::End(ref e)) => {
                let tag_name = e.name();
                let tag_bytes = tag_name.as_ref();
                let local = local_name(tag_bytes);
                let prefix = namespace_prefix(tag_bytes);
                let local_str = String::from_utf8_lossy(local).to_string();
                let is_gml = prefix.is_some_and(|p| p == NS_GML.as_bytes());

                // Handle envelope corners
                if is_gml && local == b"lowerCorner" && state.in_lower_corner {
                    state.envelope_lower = parse_pos(&state.text_buf).ok();
                    state.in_lower_corner = false;
                } else if is_gml && local == b"upperCorner" && state.in_upper_corner {
                    state.envelope_upper = parse_pos(&state.text_buf).ok();
                    state.in_upper_corner = false;
                } else if is_gml && local == b"Envelope" {
                    state.in_envelope = false;
                } else if is_gml && local == b"boundedBy" {
                    state.in_bounded_by = false;
                }

                // Handle coordinate elements
                if state.in_feature && is_gml {
                    match local {
                        b"pos" => {
                            if let Ok(pt) = parse_pos(&state.text_buf) {
                                state.points.push(pt);
                            }
                        }
                        b"posList" => {
                            if let Ok(pts) = parse_pos_list(&state.text_buf) {
                                state.points.extend(pts);
                            }
                        }
                        b"coordinates" => {
                            if let Ok(pts) = parse_coordinates(&state.text_buf) {
                                state.points.extend(pts);
                            }
                        }
                        b"Point" => {
                            // Point geometry complete -- points should have 1 entry
                            if state.geom_state == GeomState::Point {
                                // Will be finalized on feature end
                            }
                        }
                        b"LineString" => {
                            match state.geom_state {
                                GeomState::MultiCurve => {
                                    // Inside curveMember
                                    if !state.points.is_empty() {
                                        state.line_parts.push(std::mem::take(&mut state.points));
                                    }
                                }
                                _ => {
                                    // Single LineString -- finalized on feature end
                                }
                            }
                        }
                        b"Curve" => {
                            if state.geom_state == GeomState::MultiCurve && !state.points.is_empty()
                            {
                                state.line_parts.push(std::mem::take(&mut state.points));
                            }
                        }
                        b"LinearRing" => {
                            // Collect ring points
                            if state.in_exterior {
                                state.exterior_ring = Some(std::mem::take(&mut state.points));
                            } else if state.in_interior {
                                state.interior_rings.push(std::mem::take(&mut state.points));
                            }
                        }
                        b"exterior" => state.in_exterior = false,
                        b"interior" => state.in_interior = false,
                        b"Polygon" | b"Surface" => {
                            if state.geom_state == GeomState::MultiSurface {
                                // Finalize one polygon in multi
                                let polygon = build_polygon(
                                    state.exterior_ring.take(),
                                    std::mem::take(&mut state.interior_rings),
                                );
                                if let Some(poly) = polygon {
                                    state.surface_polygons.push(poly);
                                }
                            }
                            // Single polygon finalized on feature end
                        }
                        b"curveMember" => state.in_curve_member = false,
                        b"surfaceMember" => state.in_surface_member = false,
                        _ => {}
                    }
                }

                // Capture attribute values (non-gml elements that are direct children of the feature)
                if state.in_feature
                    && !is_gml
                    && state.path.len() == state.feature_depth + 1
                    && Some(&local_str) != state.feature_type.as_ref()
                {
                    // Only capture if the text is not empty or geometry-related
                    // Skip geometry wrapper elements (they contain child elements, not text)
                    let trimmed = state.text_buf.trim();
                    if !trimmed.is_empty() {
                        let val = parse_attribute_value(trimmed);
                        state.attributes.insert(local_str.clone(), val);
                    }
                }

                // End of feature member
                if is_gml && (local == b"featureMember" || local == b"featureMembers") {
                    if state.in_feature {
                        finalize_feature(&mut state);
                    }
                    state.in_feature_member = false;
                    state.reset_feature();
                }

                state.path.pop();
                state.text_buf.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(ShapefileError::GmlXmlError(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    // Build the bounding box
    let bbox = match (state.envelope_lower, state.envelope_upper) {
        (Some(lower), Some(upper)) => BoundingBox {
            x_min: lower.x,
            y_min: lower.y,
            x_max: upper.x,
            y_max: upper.y,
        },
        _ => compute_bbox_from_records(&state.records),
    };

    Ok((state.records, bbox, state.srs_name))
}

/// Extracts `srsName` from a geometry element's attributes if not yet captured.
fn extract_srs_name(e: &quick_xml::events::BytesStart<'_>, state: &mut ParseState) {
    if state.srs_captured {
        return;
    }
    for attr in e.attributes().flatten() {
        if local_name(attr.key.as_ref()) == b"srsName" {
            state.srs_name = Some(String::from_utf8_lossy(&attr.value).to_string());
            state.srs_captured = true;
        }
    }
}

/// Builds a `Polygon` from exterior and interior rings.
fn build_polygon(exterior: Option<Vec<Point>>, interiors: Vec<Vec<Point>>) -> Option<Polygon> {
    let ext = exterior?;
    if ext.is_empty() {
        return None;
    }
    let mut rings = vec![Ring { points: ext }];
    for interior in interiors {
        if !interior.is_empty() {
            rings.push(Ring { points: interior });
        }
    }
    Some(Polygon { rings })
}

/// Finalizes the current feature and pushes records to the state.
fn finalize_feature(state: &mut ParseState) {
    let attributes = state.attributes.clone();

    match &state.geom_state {
        GeomState::Point => {
            if let Some(pt) = state.points.first().copied() {
                state.records.push(ShapeRecord {
                    record_number: state.next_record_number,
                    geometry: Geometry::Point(pt),
                    attributes,
                });
                state.next_record_number += 1;
            }
        }
        GeomState::LineString | GeomState::Curve => {
            if !state.points.is_empty() {
                state.records.push(ShapeRecord {
                    record_number: state.next_record_number,
                    geometry: Geometry::Polyline(Polyline {
                        parts: vec![std::mem::take(&mut state.points)],
                    }),
                    attributes,
                });
                state.next_record_number += 1;
            }
        }
        GeomState::MultiCurve => {
            // Any remaining points go as a part
            if !state.points.is_empty() {
                state.line_parts.push(std::mem::take(&mut state.points));
            }
            if !state.line_parts.is_empty() {
                state.records.push(ShapeRecord {
                    record_number: state.next_record_number,
                    geometry: Geometry::Polyline(Polyline {
                        parts: std::mem::take(&mut state.line_parts),
                    }),
                    attributes,
                });
                state.next_record_number += 1;
            }
        }
        GeomState::Polygon | GeomState::Surface => {
            let polygon = build_polygon(
                state.exterior_ring.take(),
                std::mem::take(&mut state.interior_rings),
            );
            if let Some(poly) = polygon {
                state.records.push(ShapeRecord {
                    record_number: state.next_record_number,
                    geometry: Geometry::Polygon(poly),
                    attributes,
                });
                state.next_record_number += 1;
            }
        }
        GeomState::MultiSurface => {
            // Any remaining polygon not yet finalized
            let polygon = build_polygon(
                state.exterior_ring.take(),
                std::mem::take(&mut state.interior_rings),
            );
            if let Some(poly) = polygon {
                state.surface_polygons.push(poly);
            }
            // Expand to multiple records (like GeoJSON MultiPolygon)
            for poly in std::mem::take(&mut state.surface_polygons) {
                state.records.push(ShapeRecord {
                    record_number: state.next_record_number,
                    geometry: Geometry::Polygon(poly),
                    attributes: attributes.clone(),
                });
                state.next_record_number += 1;
            }
        }
        GeomState::None => {
            // Feature with no geometry
            state.records.push(ShapeRecord {
                record_number: state.next_record_number,
                geometry: Geometry::Null,
                attributes,
            });
            state.next_record_number += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute capture — re-entry after geometry elements
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// GmlReader
// ---------------------------------------------------------------------------

/// A reader for GML (Geography Markup Language) files, specifically targeting
/// the GML profile used by JPGIS2.1 / KSJ-style datasets.
///
/// All records are loaded into memory at construction time, so accessor methods
/// return borrowed references rather than owned values.
///
/// # Feature support
///
/// - `gml:Point`, `gml:LineString`, `gml:Curve`
/// - `gml:Polygon`, `gml:Surface`
/// - `gml:MultiCurve` (combined into single `Polyline` with multiple parts)
/// - `gml:MultiSurface` (expanded into multiple `Polygon` records)
/// - `gml:boundedBy` / `gml:Envelope` for bounding box
/// - `srsName` extraction from geometry elements
///
/// `GeometryCollection` is **not** supported and will return an error.
pub struct GmlReader {
    records: Vec<ShapeRecord>,
    bbox: BoundingBox,
    srs_name: Option<String>,
}

impl GmlReader {
    /// Opens a GML file at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ShapefileError> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        Self::from_reader(reader)
    }

    /// Reads GML from any `BufRead` source.
    pub fn from_reader<R: BufRead>(reader: R) -> Result<Self, ShapefileError> {
        let (records, bbox, srs_name) = parse_gml(reader)?;
        Ok(Self {
            records,
            bbox,
            srs_name,
        })
    }

    /// Parses a GML string into a `GmlReader`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(xml: &str) -> Result<Self, ShapefileError> {
        Self::from_reader(xml.as_bytes())
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

    /// Returns the SRS name extracted from the first geometry element, if any.
    pub fn srs_name(&self) -> Option<&str> {
        self.srs_name.as_deref()
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

    /// Filters records where a `Text` attribute starts with the given prefix.
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
    /// This method is only available when both `gml` and `geojson` features are enabled.
    #[cfg(feature = "geojson")]
    pub fn to_geojson(&self, limit: Option<usize>) -> Result<String, ShapefileError> {
        use serde_json::{json, Value};

        let recs = self.records(limit);
        let features: Vec<Value> = recs
            .into_iter()
            .map(|r| {
                let geometry = geometry_to_geojson(&r.geometry);
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

/// Converts a `Geometry` to a GeoJSON `Value`.
#[cfg(feature = "geojson")]
fn geometry_to_geojson(geom: &Geometry) -> serde_json::Value {
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

    // --- parse_pos_list ---

    #[test]
    fn test_parse_pos_list_valid() {
        let pts = parse_pos_list("35.0 139.0 36.0 140.0").unwrap();
        assert_eq!(pts.len(), 2);
        assert!((pts[0].x - 139.0).abs() < 1e-10);
        assert!((pts[0].y - 35.0).abs() < 1e-10);
        assert!((pts[1].x - 140.0).abs() < 1e-10);
        assert!((pts[1].y - 36.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_pos_list_odd_count() {
        let result = parse_pos_list("35.0 139.0 36.0");
        assert!(result.is_err());
        if let Err(ShapefileError::InvalidGml { reason }) = result {
            assert!(reason.contains("odd"));
        }
    }

    #[test]
    fn test_parse_pos_list_invalid_float() {
        let result = parse_pos_list("35.0 abc");
        assert!(result.is_err());
        if let Err(ShapefileError::InvalidGml { reason }) = result {
            assert!(reason.contains("invalid float"));
        }
    }

    // --- parse_pos ---

    #[test]
    fn test_parse_pos_valid() {
        let pt = parse_pos("35.6895 139.6917").unwrap();
        assert!((pt.x - 139.6917).abs() < 1e-10);
        assert!((pt.y - 35.6895).abs() < 1e-10);
    }

    // --- local_name ---

    #[test]
    fn test_local_name_with_prefix() {
        assert_eq!(local_name(b"gml:Point"), b"Point");
    }

    #[test]
    fn test_local_name_without_prefix() {
        assert_eq!(local_name(b"Road"), b"Road");
    }

    // --- Point element ---

    #[test]
    fn test_parse_point_element() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Station gml:id="st1">
      <ksj:location>
        <gml:Point srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:pos>35.6895 139.6917</gml:pos>
        </gml:Point>
      </ksj:location>
      <ksj:stationName>Tokyo</ksj:stationName>
    </ksj:Station>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        assert_eq!(reader.len(), 1);
        let rec = reader.get(0).unwrap();
        if let Geometry::Point(p) = &rec.geometry {
            assert!((p.x - 139.6917).abs() < 1e-4);
            assert!((p.y - 35.6895).abs() < 1e-4);
        } else {
            panic!("expected Point geometry");
        }
    }

    // --- LineString ---

    #[test]
    fn test_parse_linestring() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Road gml:id="r1">
      <ksj:location>
        <gml:LineString srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:posList>35.0 139.0 35.1 139.1 35.2 139.2</gml:posList>
        </gml:LineString>
      </ksj:location>
    </ksj:Road>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        assert_eq!(reader.len(), 1);
        let rec = reader.get(0).unwrap();
        if let Geometry::Polyline(pl) = &rec.geometry {
            assert_eq!(pl.parts.len(), 1);
            assert_eq!(pl.parts[0].len(), 3);
        } else {
            panic!("expected Polyline geometry");
        }
    }

    // --- Polygon exterior only ---

    #[test]
    fn test_parse_polygon_exterior_only() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Area gml:id="a1">
      <ksj:location>
        <gml:Polygon srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:exterior>
            <gml:LinearRing>
              <gml:posList>35.0 139.0 35.1 139.0 35.1 139.1 35.0 139.1 35.0 139.0</gml:posList>
            </gml:LinearRing>
          </gml:exterior>
        </gml:Polygon>
      </ksj:location>
    </ksj:Area>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        assert_eq!(reader.len(), 1);
        let rec = reader.get(0).unwrap();
        if let Geometry::Polygon(pg) = &rec.geometry {
            assert_eq!(pg.rings.len(), 1);
            assert_eq!(pg.exterior().points.len(), 5);
            assert!(pg.holes().is_empty());
        } else {
            panic!("expected Polygon geometry");
        }
    }

    // --- Polygon with hole ---

    #[test]
    fn test_parse_polygon_with_hole() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Area gml:id="a1">
      <ksj:location>
        <gml:Polygon srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:exterior>
            <gml:LinearRing>
              <gml:posList>35.0 139.0 35.1 139.0 35.1 139.1 35.0 139.1 35.0 139.0</gml:posList>
            </gml:LinearRing>
          </gml:exterior>
          <gml:interior>
            <gml:LinearRing>
              <gml:posList>35.02 139.02 35.08 139.02 35.08 139.08 35.02 139.08 35.02 139.02</gml:posList>
            </gml:LinearRing>
          </gml:interior>
        </gml:Polygon>
      </ksj:location>
    </ksj:Area>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        assert_eq!(reader.len(), 1);
        let rec = reader.get(0).unwrap();
        if let Geometry::Polygon(pg) = &rec.geometry {
            assert_eq!(pg.rings.len(), 2);
            assert_eq!(pg.holes().len(), 1);
        } else {
            panic!("expected Polygon geometry");
        }
    }

    // --- MultiCurve ---

    #[test]
    fn test_parse_multilinestring() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Road gml:id="r1">
      <ksj:location>
        <gml:MultiCurve srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:curveMember>
            <gml:LineString>
              <gml:posList>35.0 139.0 35.1 139.1</gml:posList>
            </gml:LineString>
          </gml:curveMember>
          <gml:curveMember>
            <gml:LineString>
              <gml:posList>36.0 140.0 36.1 140.1</gml:posList>
            </gml:LineString>
          </gml:curveMember>
        </gml:MultiCurve>
      </ksj:location>
    </ksj:Road>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        assert_eq!(reader.len(), 1);
        let rec = reader.get(0).unwrap();
        if let Geometry::Polyline(pl) = &rec.geometry {
            assert_eq!(pl.parts.len(), 2);
        } else {
            panic!("expected Polyline geometry");
        }
    }

    // --- MultiSurface expansion ---

    #[test]
    fn test_parse_multipolygon_expansion() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Area gml:id="a1">
      <ksj:location>
        <gml:MultiSurface srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:surfaceMember>
            <gml:Polygon>
              <gml:exterior>
                <gml:LinearRing>
                  <gml:posList>35.0 139.0 35.1 139.0 35.1 139.1 35.0 139.1 35.0 139.0</gml:posList>
                </gml:LinearRing>
              </gml:exterior>
            </gml:Polygon>
          </gml:surfaceMember>
          <gml:surfaceMember>
            <gml:Polygon>
              <gml:exterior>
                <gml:LinearRing>
                  <gml:posList>36.0 140.0 36.1 140.0 36.1 140.1 36.0 140.1 36.0 140.0</gml:posList>
                </gml:LinearRing>
              </gml:exterior>
            </gml:Polygon>
          </gml:surfaceMember>
        </gml:MultiSurface>
      </ksj:location>
      <ksj:name>test</ksj:name>
    </ksj:Area>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        // MultiSurface expands to 2 records
        assert_eq!(reader.len(), 2);
        for rec in reader.iter_records() {
            assert!(matches!(rec.geometry, Geometry::Polygon(_)));
        }
    }

    // --- Attribute parsing ---

    #[test]
    fn test_parse_attr_numeric() {
        let val = parse_attribute_value("42");
        assert_eq!(val, AttributeValue::Numeric(42.0));
    }

    #[test]
    fn test_parse_attr_string() {
        let val = parse_attribute_value("Tokyo");
        assert_eq!(val, AttributeValue::Text("Tokyo".to_string()));
    }

    #[test]
    fn test_parse_attr_empty() {
        let val = parse_attribute_value("");
        assert_eq!(val, AttributeValue::Null);
    }

    // --- bbox from boundedBy ---

    #[test]
    fn test_bbox_from_bounded_by() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:boundedBy>
    <gml:Envelope srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
      <gml:lowerCorner>34.0 138.0</gml:lowerCorner>
      <gml:upperCorner>36.0 140.0</gml:upperCorner>
    </gml:Envelope>
  </gml:boundedBy>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        let bb = reader.bbox();
        // lowerCorner: lat=34 lon=138 → x_min=138, y_min=34
        // upperCorner: lat=36 lon=140 → x_max=140, y_max=36
        assert!((bb.x_min - 138.0).abs() < 1e-10);
        assert!((bb.y_min - 34.0).abs() < 1e-10);
        assert!((bb.x_max - 140.0).abs() < 1e-10);
        assert!((bb.y_max - 36.0).abs() < 1e-10);
    }

    // --- bbox computed from records ---

    #[test]
    fn test_bbox_computed_from_records() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Station gml:id="s1">
      <ksj:location>
        <gml:Point><gml:pos>35.0 139.0</gml:pos></gml:Point>
      </ksj:location>
    </ksj:Station>
  </gml:featureMember>
  <gml:featureMember>
    <ksj:Station gml:id="s2">
      <ksj:location>
        <gml:Point><gml:pos>36.0 140.0</gml:pos></gml:Point>
      </ksj:location>
    </ksj:Station>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        let bb = reader.bbox();
        assert!((bb.x_min - 139.0).abs() < 1e-10);
        assert!((bb.y_min - 35.0).abs() < 1e-10);
        assert!((bb.x_max - 140.0).abs() < 1e-10);
        assert!((bb.y_max - 36.0).abs() < 1e-10);
    }

    // --- srsName extraction ---

    #[test]
    fn test_srs_name_extracted() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Station gml:id="s1">
      <ksj:location>
        <gml:Point srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:pos>35.0 139.0</gml:pos>
        </gml:Point>
      </ksj:location>
    </ksj:Station>
  </gml:featureMember>
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        assert_eq!(
            reader.srs_name(),
            Some("http://www.opengis.net/def/crs/EPSG/0/6668")
        );
    }

    // --- Empty FeatureCollection ---

    #[test]
    fn test_empty_feature_collection() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
</ksj:Dataset>"#;

        let reader = GmlReader::from_str(xml).unwrap();
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
    }

    // --- Unsupported GeometryCollection ---

    #[test]
    fn test_unsupported_geometry_collection() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
             xmlns:gml="http://www.opengis.net/gml">
  <gml:featureMember>
    <ksj:Thing gml:id="t1">
      <ksj:location>
        <gml:GeometryCollection/>
      </ksj:location>
    </ksj:Thing>
  </gml:featureMember>
</ksj:Dataset>"#;

        let result = GmlReader::from_str(xml);
        assert!(result.is_err());
        if let Err(ShapefileError::InvalidGml { reason }) = result {
            assert!(reason.contains("GeometryCollection"));
        }
    }
}
