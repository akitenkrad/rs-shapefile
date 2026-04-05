//! A pure-Rust library for reading ESRI Shapefiles and GeoJSON.
//!
//! - **Shapefile**: reads `.shp`, `.shx`, `.dbf`, `.prj`, and `.cpg` via [`ShapefileReader`]
//! - **GeoJSON**: reads RFC 7946 GeoJSON via [`GeoJsonReader`] (requires `geojson` feature)
//!
//! Both readers share the same model types ([`Geometry`], [`ShapeRecord`], [`BoundingBox`]),
//! enabling format-agnostic analysis code.

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

#[cfg(feature = "geojson")]
pub mod geojson_reader;

#[cfg(feature = "geojson")]
pub use crate::geojson_reader::GeoJsonReader;

#[cfg(feature = "mvt")]
pub mod mvt_reader;

#[cfg(feature = "mvt")]
pub use crate::mvt_reader::{LayerFilter, MvtReader, TileCoord};
