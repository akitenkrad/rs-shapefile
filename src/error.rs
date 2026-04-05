use thiserror::Error;

/// Errors that can occur when reading or processing shapefiles.
#[derive(Debug, Error)]
pub enum ShapefileError {
    /// Underlying I/O error from file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A required companion file (.shp or .dbf) was not found.
    #[error("required file not found: {path}")]
    MissingFile { path: std::path::PathBuf },

    /// The .shp file header contains an invalid file code (expected 9994).
    #[error("invalid file code: expected 9994, got {actual}")]
    InvalidFileCode { actual: i32 },

    /// The .shp file header contains an invalid version (expected 1000).
    #[error("invalid version: expected 1000, got {actual}")]
    InvalidVersion { actual: i32 },

    /// The file data is corrupted or truncated.
    #[error("corrupted file: {reason}")]
    CorruptedFile { reason: String },

    /// An unrecognized shape type code was encountered.
    #[error("unsupported shape type: {0}")]
    UnsupportedShapeType(i32),

    /// Parsed geometry is structurally invalid.
    #[error("invalid geometry: {reason}")]
    InvalidGeometry { reason: String },

    /// Character encoding error while reading a DBF field value.
    #[error("encoding error in field '{field}': {reason}")]
    EncodingError { field: String, reason: String },

    /// The requested attribute field does not exist in the DBF header.
    #[error("field '{0}' not found in attribute table")]
    FieldNotFound(String),

    /// `describe()` was called on a non-numeric field (Character, Date, or Logical).
    #[error("describe() called on non-numeric field '{field}' (type: {field_type})")]
    DescribeOnNonNumericField { field: String, field_type: String },

    /// An attribute value's type does not match the expected field type.
    #[error("type mismatch in field '{field}': expected {expected}, got {actual}")]
    TypeMismatch {
        field: String,
        expected: String,
        actual: String,
    },

    /// GeoJSON serialization failed (requires `geojson` feature).
    #[cfg(feature = "geojson")]
    #[error("GeoJSON serialization error: {0}")]
    GeoJsonError(#[from] serde_json::Error),

    /// The GeoJSON input is structurally invalid or contains unsupported types.
    #[cfg(feature = "geojson")]
    #[error("invalid GeoJSON: {reason}")]
    InvalidGeoJson { reason: String },

    /// Protobuf decoding failed when reading an MVT file (requires `mvt` feature).
    #[cfg(feature = "mvt")]
    #[error("MVT decode error: {0}")]
    MvtDecodeError(#[from] prost::DecodeError),

    /// The MVT data is structurally invalid or contains unsupported values.
    #[cfg(feature = "mvt")]
    #[error("invalid MVT: {reason}")]
    InvalidMvt { reason: String },

    /// GML XML parse error (requires `gml` feature).
    #[cfg(feature = "gml")]
    #[error("GML XML parse error: {0}")]
    GmlXmlError(String),

    /// The GML input is structurally invalid or contains unsupported types.
    #[cfg(feature = "gml")]
    #[error("invalid GML: {reason}")]
    InvalidGml { reason: String },
}
