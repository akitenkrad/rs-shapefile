//! A pure-Rust Shapefile reader for GIS data analysis.
//!
//! Reads `.shp`, `.shx`, `.dbf`, `.prj`, and `.cpg` files with streaming support,
//! spatial/attribute filtering, and optional GeoJSON export.

pub mod error;
pub mod reader;

pub(crate) mod io;
pub(crate) mod models;
pub(crate) mod parsers;

pub use crate::error::ShapefileError;
pub use crate::reader::ShapeRecordIter;
pub use crate::reader::ShapefileReader;

pub use crate::models::attribute::{AttributeValue, FieldDef, FieldStats, FieldType};
pub use crate::models::bbox::BoundingBox;
pub use crate::models::crs::Crs;
pub use crate::models::geometry::{
    Geometry, HasGeometry, MultiPoint, Point, PointM, PointZ, Polygon, PolygonZ, Polyline,
    PolylineM, PolylineZ, Ring,
};
pub use crate::models::record::ShapeRecord;

// ShapeType is frequently used, re-export directly
pub use crate::parsers::shp_parser::ShapeType;
