# rs-shapefile

A pure-Rust library for reading ESRI Shapefiles. Parses `.shp`, `.shx`, `.dbf`, `.prj`, and `.cpg` files with streaming support, spatial/attribute filtering, and optional GeoJSON export.

## Features

- **Full Shapefile support** -- reads geometry (`.shp`), index (`.shx`), attributes (`.dbf`), projection (`.prj`), and encoding (`.cpg`)
- **Streaming iteration** -- `iter_records()` processes records one at a time for memory-efficient handling of large files
- **Spatial filtering** -- `filter_by_bbox()` for bounding-box intersection queries
- **Attribute filtering** -- exact match, IN-list, and prefix match on field values
- **Descriptive statistics** -- `describe()` computes count, min, max, mean, and median for numeric fields
- **GeoJSON export** -- `to_geojson()` serializes records as a GeoJSON FeatureCollection (requires `geojson` feature)
- **Automatic `.shx` fallback** -- sequential scan when the index file is missing

## Quick Start

```rust
use rs_shapefile::{ShapefileReader, BoundingBox, Geometry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sf = ShapefileReader::open("example.shp")?;

    // Metadata
    println!("Shape type: {:?}", sf.shape_type());
    println!("Records: {}", sf.len());
    println!("BBox: {:?}", sf.bbox());

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

    // Statistics
    let stats = sf.describe("population")?;
    println!("mean={:.1}, median={:.1}", stats.mean, stats.median);

    // CRS info
    if let Some(crs) = sf.crs() {
        println!("CRS: {}", crs.name().unwrap_or("unknown"));
    }

    Ok(())
}
```

## Feature Flags

| Flag | Description | Default |
|------|-------------|---------|
| `geojson` | Enables `to_geojson()` export (adds `serde` + `serde_json`) | off |

```toml
[dependencies]
rs-shapefile = { version = "0.1", features = ["geojson"] }
```

## Minimum Supported Rust Version

**1.75.0**