# rs-shapefile

A pure-Rust library for reading ESRI Shapefiles and GeoJSON. Provides spatial/attribute filtering, descriptive statistics, and GeoJSON import/export through a shared model layer.

## Features

- **Shapefile reader** -- reads `.shp`, `.shx`, `.dbf`, `.prj`, and `.cpg` with automatic `.shx` fallback
- **GeoJSON reader** -- reads RFC 7946 `FeatureCollection`, single `Feature`, and bare `Geometry` (requires `geojson` feature)
- **Shared model types** -- both readers produce the same `Geometry`, `ShapeRecord`, and `BoundingBox`, enabling format-agnostic analysis
- **Streaming iteration** -- `ShapefileReader::iter_records()` processes records one at a time for large files
- **Spatial filtering** -- `filter_by_bbox()` for bounding-box intersection queries
- **Attribute filtering** -- exact match, IN-list, and prefix match on field values
- **Descriptive statistics** -- `describe()` computes count, min, max, mean, and median for numeric fields
- **GeoJSON export** -- `to_geojson()` serializes records as a GeoJSON FeatureCollection

## Quick Start -- Shapefile

```rust
use rs_shapefile::{ShapefileReader, BoundingBox, Geometry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sf = ShapefileReader::open("example.shp")?;

    println!("Shape type: {:?}", sf.shape_type());
    println!("Records: {}", sf.len());

    // Spatial filter
    let results = sf.filter_by_bbox(&BoundingBox {
        x_min: 139.68, y_min: 35.68,
        x_max: 139.71, y_max: 35.71,
    })?;

    // Streaming iteration
    for record in sf.iter_records() {
        let record = record?;
        if let Geometry::Polyline(line) = &record.geometry {
            println!("length = {}", line.length());
        }
    }

    Ok(())
}
```

## Quick Start -- GeoJSON

```rust
use rs_shapefile::GeoJsonReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = GeoJsonReader::open("data.geojson")?;

    println!("Records: {}", reader.len());
    println!("BBox: {:?}", reader.bbox());

    // Same filtering/statistics API as ShapefileReader
    let filtered = reader.filter_by_bbox(reader.bbox());
    let stats = reader.describe("population")?;
    println!("mean={:.1}, median={:.1}", stats.mean, stats.median);

    // Can also parse from a string
    let reader2 = GeoJsonReader::from_str(r#"{
        "type": "FeatureCollection",
        "features": []
    }"#)?;

    Ok(())
}
```

## Feature Flags

| Flag | Description | Default |
|------|-------------|---------|
| `geojson` | Enables `GeoJsonReader`, `to_geojson()` export, and GeoJSON import (adds `serde` + `serde_json`) | off |

```toml
[dependencies]
rs-shapefile = { version = "0.1", features = ["geojson"] }
```

## Minimum Supported Rust Version

**1.75.0**
