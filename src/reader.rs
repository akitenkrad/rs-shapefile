use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::error::ShapefileError;
use crate::models::attribute::{AttributeValue, FieldDef, FieldStats};
use crate::models::bbox::BoundingBox;
use crate::models::crs::Crs;
use crate::models::geometry::Geometry;
use crate::models::record::ShapeRecord;
use crate::parsers::cpg_parser::CpgParser;
use crate::parsers::dbf_parser::DbfParser;
use crate::parsers::prj_parser::PrjParser;
use crate::parsers::shp_parser::{ShapeType, ShpParser};

/// Main entry point for reading ESRI Shapefiles (.shp/.shx/.dbf/.prj/.cpg).
pub struct ShapefileReader {
    shape_type: ShapeType,
    bbox: BoundingBox,
    crs: Option<Crs>,
    num_records: usize,
    shp_parser: ShpParser<BufReader<File>>,
    dbf_parser: DbfParser<BufReader<File>>,
    dbf_fields: Vec<FieldDef>,
}

/// Streaming iterator over shape records, yielding one record at a time.
pub struct ShapeRecordIter<'a> {
    reader: &'a mut ShapefileReader,
    record_index: u32,
}

impl Iterator for ShapeRecordIter<'_> {
    type Item = Result<ShapeRecord, ShapefileError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.record_index as usize >= self.reader.num_records {
            return None;
        }

        // Parse next geometry
        let geom = match self.reader.shp_parser.parse_record() {
            Ok(g) => g,
            Err(e) => return Some(Err(e)),
        };

        // Parse next attributes
        let attrs_vec = match self.reader.dbf_parser.iter_records().next() {
            Some(Ok(a)) => a,
            Some(Err(e)) => return Some(Err(e)),
            None => return None,
        };

        let record_number = self.record_index + 1;
        self.record_index += 1;

        let attributes: HashMap<String, AttributeValue> = attrs_vec.into_iter().collect();

        Some(Ok(ShapeRecord {
            record_number,
            geometry: geom,
            attributes,
        }))
    }
}

impl ShapefileReader {
    /// Open a shapefile by .shp path (other files are auto-discovered)
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ShapefileError> {
        let shp_path = path.as_ref().to_path_buf();

        // Check .shp exists
        if !shp_path.exists() {
            return Err(ShapefileError::MissingFile { path: shp_path });
        }

        // Derive companion paths
        let stem = shp_path.with_extension("");
        let dbf_path = stem.with_extension("dbf");
        let shx_path = stem.with_extension("shx");
        let cpg_path = stem.with_extension("cpg");
        let prj_path = stem.with_extension("prj");

        // Check .dbf exists
        if !dbf_path.exists() {
            return Err(ShapefileError::MissingFile { path: dbf_path });
        }

        // Parse .cpg if exists
        let encoding = if cpg_path.exists() {
            let cpg_file = File::open(&cpg_path)?;
            let mut cpg_parser = CpgParser::new(BufReader::new(cpg_file));
            Some(cpg_parser.parse()?)
        } else {
            None
        };

        // Parse .prj if exists
        let crs = if prj_path.exists() {
            let prj_file = File::open(&prj_path)?;
            let mut prj_parser = PrjParser::new(BufReader::new(prj_file));
            Some(prj_parser.parse()?)
        } else {
            None
        };

        // Open .shp and optionally .shx
        let shp_file = BufReader::new(File::open(&shp_path)?);
        let shx_file = if shx_path.exists() {
            Some(BufReader::new(File::open(&shx_path)?))
        } else {
            None
        };

        // Create ShpParser and parse header
        let mut shp_parser = ShpParser::new(shp_file, shx_file)?;
        let shp_header = shp_parser.parse_header()?;

        // Create DbfParser and parse header
        let dbf_file = BufReader::new(File::open(&dbf_path)?);
        let mut dbf_parser = DbfParser::new(dbf_file, encoding)?;
        let dbf_header = dbf_parser.parse_header()?;

        let num_records = dbf_header.num_records as usize;
        let dbf_fields = dbf_header.fields;

        Ok(ShapefileReader {
            shape_type: shp_header.shape_type,
            bbox: shp_header.bbox,
            crs,
            num_records,
            shp_parser,
            dbf_parser,
            dbf_fields,
        })
    }

    /// Returns the shape type declared in the .shp file header.
    pub fn shape_type(&self) -> ShapeType {
        self.shape_type
    }

    /// Returns the coordinate reference system parsed from .prj, if available.
    pub fn crs(&self) -> Option<&Crs> {
        self.crs.as_ref()
    }

    /// Returns the file-level bounding box from the .shp header.
    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    /// Returns the total number of records in the shapefile.
    pub fn len(&self) -> usize {
        self.num_records
    }

    /// Returns `true` if the shapefile contains no records.
    pub fn is_empty(&self) -> bool {
        self.num_records == 0
    }

    /// Reads all records (or up to `limit`) into a `Vec`. For large files, prefer [`iter_records`](Self::iter_records).
    pub fn records(&mut self, limit: Option<usize>) -> Result<Vec<ShapeRecord>, ShapefileError> {
        // Reset both parsers to start of records
        self.shp_parser.seek_to_records()?;
        self.dbf_parser.seek_to_records()?;

        let max = limit.unwrap_or(self.num_records).min(self.num_records);
        let mut results = Vec::with_capacity(max);

        for i in 0..max {
            let geom = self.shp_parser.parse_record()?;
            let attrs_vec = self.dbf_parser.iter_records().next().ok_or_else(|| {
                ShapefileError::CorruptedFile {
                    reason: format!("expected DBF record at index {i}, got EOF"),
                }
            })??;

            let attributes: HashMap<String, AttributeValue> = attrs_vec.into_iter().collect();
            results.push(ShapeRecord {
                record_number: (i + 1) as u32,
                geometry: geom,
                attributes,
            });
        }

        Ok(results)
    }

    /// Returns a streaming iterator over all records. Recommended for large files.
    pub fn iter_records(&mut self) -> ShapeRecordIter<'_> {
        // Reset parsers before iteration (ignore errors — next() will surface them)
        let _ = self.shp_parser.seek_to_records();
        let _ = self.dbf_parser.seek_to_records();
        ShapeRecordIter {
            reader: self,
            record_index: 0,
        }
    }

    /// Returns the record at the given zero-based index, or `None` if out of range.
    pub fn get(&mut self, index: usize) -> Result<Option<ShapeRecord>, ShapefileError> {
        if index >= self.num_records {
            return Ok(None);
        }

        // Get geometry
        let geom = if self.shp_parser.has_shx() {
            // Random access via SHX index
            match self.shp_parser.read_at(index)? {
                Some(g) => g,
                None => return Ok(None),
            }
        } else {
            // Sequential scan: reset and skip to index
            self.shp_parser.seek_to_records()?;
            let mut geom = Geometry::Null;
            for _ in 0..=index {
                geom = self.shp_parser.parse_record()?;
            }
            geom
        };

        // Get attributes: seek to record position in DBF
        // DBF header size = 32 + (num_fields * 32) + 1
        // Record size = 1 (deletion flag) + sum of field lengths
        // Seek DBF to the target record and read one record
        self.dbf_parser.seek_to_records()?;
        // We need to skip `index` records, then read one
        // Actually, let's use a more direct approach: seek to the exact byte offset
        // But DbfParser doesn't expose raw seek. Let's iterate and skip.
        let mut attrs_vec = None;
        for (count, result) in self.dbf_parser.iter_records().enumerate() {
            if count == index {
                attrs_vec = Some(result?);
                break;
            }
            let _ = result?;
        }

        match attrs_vec {
            Some(a) => {
                let attributes: HashMap<String, AttributeValue> = a.into_iter().collect();
                Ok(Some(ShapeRecord {
                    record_number: (index + 1) as u32,
                    geometry: geom,
                    attributes,
                }))
            }
            None => Ok(None),
        }
    }

    /// Returns records whose attribute `field` exactly matches `value`.
    pub fn filter_by_attribute(
        &mut self,
        field: &str,
        value: &AttributeValue,
    ) -> Result<Vec<ShapeRecord>, ShapefileError> {
        self.validate_field_exists(field)?;
        let all = self.records(None)?;
        Ok(all
            .into_iter()
            .filter(|r| r.attributes.get(field) == Some(value))
            .collect())
    }

    /// Returns records whose attribute `field` matches any of the given `values`.
    pub fn filter_by_attribute_in(
        &mut self,
        field: &str,
        values: &[AttributeValue],
    ) -> Result<Vec<ShapeRecord>, ShapefileError> {
        self.validate_field_exists(field)?;
        let all = self.records(None)?;
        Ok(all
            .into_iter()
            .filter(|r| {
                r.attributes
                    .get(field)
                    .map(|v| values.contains(v))
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Returns records whose attribute `field` starts with the given `prefix` (Text fields only).
    pub fn filter_by_attribute_starts_with(
        &mut self,
        field: &str,
        prefix: &str,
    ) -> Result<Vec<ShapeRecord>, ShapefileError> {
        self.validate_field_exists(field)?;
        let all = self.records(None)?;
        Ok(all
            .into_iter()
            .filter(|r| {
                r.attributes
                    .get(field)
                    .map(|v| v.starts_with(prefix))
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Returns records whose bounding box intersects the given `bbox`.
    pub fn filter_by_bbox(
        &mut self,
        bbox: &BoundingBox,
    ) -> Result<Vec<ShapeRecord>, ShapefileError> {
        let all = self.records(None)?;
        Ok(all
            .into_iter()
            .filter(|r| {
                r.geometry
                    .bbox()
                    .map(|gb| gb.intersects(bbox))
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Computes descriptive statistics (count, min, max, mean, median) for a numeric field.
    pub fn describe(&mut self, field: &str) -> Result<FieldStats, ShapefileError> {
        // Find field definition
        let field_def = self
            .dbf_fields
            .iter()
            .find(|f| f.name == field)
            .ok_or_else(|| ShapefileError::FieldNotFound(field.to_string()))?;

        // Check numeric
        if !field_def.is_numeric() {
            return Err(ShapefileError::DescribeOnNonNumericField {
                field: field.to_string(),
                field_type: format!("{:?}", field_def.field_type),
            });
        }

        // Collect all numeric values
        let all = self.records(None)?;
        let mut values: Vec<f64> = all
            .iter()
            .filter_map(|r| r.attributes.get(field).and_then(|v| v.as_f64()))
            .collect();

        if values.is_empty() {
            return Ok(FieldStats {
                count: 0,
                min: f64::NAN,
                max: f64::NAN,
                mean: f64::NAN,
                median: f64::NAN,
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

    /// Exports records as a GeoJSON `FeatureCollection` string. Optionally limits the number of features.
    #[cfg(feature = "geojson")]
    pub fn to_geojson(&mut self, limit: Option<usize>) -> Result<String, ShapefileError> {
        use serde_json::{json, Value};

        let records = self.records(limit)?;
        let features: Vec<Value> = records
            .into_iter()
            .map(|r| {
                let geometry = geometry_to_geojson(&r.geometry);
                let properties: serde_json::Map<String, Value> = r
                    .attributes
                    .into_iter()
                    .map(|(k, v)| {
                        let jv = match v {
                            AttributeValue::Text(s) => Value::String(s),
                            AttributeValue::Numeric(n) => {
                                json!(n)
                            }
                            AttributeValue::Date(d) => Value::String(d),
                            AttributeValue::Logical(b) => Value::Bool(b),
                            AttributeValue::Null => Value::Null,
                        };
                        (k, jv)
                    })
                    .collect();
                json!({
                    "type": "Feature",
                    "geometry": geometry,
                    "properties": properties,
                })
            })
            .collect();

        let collection = json!({
            "type": "FeatureCollection",
            "features": features,
        });

        Ok(serde_json::to_string(&collection)?)
    }

    // Private helpers

    fn validate_field_exists(&self, field: &str) -> Result<(), ShapefileError> {
        if !self.dbf_fields.iter().any(|f| f.name == field) {
            return Err(ShapefileError::FieldNotFound(field.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Build a minimal .shp file with a header and point records.
    fn build_shp_bytes(points: &[(f64, f64)]) -> Vec<u8> {
        let mut buf = Vec::new();

        // Record bytes per point: 8 (record header) + 4 (shape type) + 16 (x,y) = 28 bytes
        let records_bytes: usize = points.len() * 28;
        let file_length_bytes = 100 + records_bytes;
        let file_length_words = file_length_bytes / 2;

        // Header (100 bytes)
        buf.extend_from_slice(&9994_i32.to_be_bytes()); // file code
        buf.extend_from_slice(&[0u8; 20]); // unused
        buf.extend_from_slice(&(file_length_words as i32).to_be_bytes()); // file length
        buf.extend_from_slice(&1000_i32.to_le_bytes()); // version
        buf.extend_from_slice(&1_i32.to_le_bytes()); // shape type = Point

        // bbox
        if points.is_empty() {
            buf.extend_from_slice(&[0u8; 64]);
        } else {
            let x_min = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let y_min = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let x_max = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let y_max = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            buf.extend_from_slice(&x_min.to_le_bytes());
            buf.extend_from_slice(&y_min.to_le_bytes());
            buf.extend_from_slice(&x_max.to_le_bytes());
            buf.extend_from_slice(&y_max.to_le_bytes());
            buf.extend_from_slice(&[0u8; 32]); // z/m ranges
        }

        // Point records
        for (i, (x, y)) in points.iter().enumerate() {
            buf.extend_from_slice(&((i + 1) as i32).to_be_bytes()); // record number
            buf.extend_from_slice(&10_i32.to_be_bytes()); // content length (20 bytes = 10 words)
            buf.extend_from_slice(&1_i32.to_le_bytes()); // shape type = Point
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
        }

        buf
    }

    /// Build a minimal .dbf file with one Character field "NAME" per record.
    fn build_dbf_bytes(names: &[&str], field_len: u8) -> Vec<u8> {
        let mut buf = Vec::new();

        let num_fields: usize = 1;
        let header_size: u16 = 32 + (num_fields as u16 * 32) + 1;
        let record_size: u16 = 1 + field_len as u16;

        // Main header (32 bytes)
        buf.push(0x03); // version
        buf.extend_from_slice(&[24, 1, 1]); // date
        buf.extend_from_slice(&(names.len() as i32).to_le_bytes());
        buf.extend_from_slice(&(header_size as i16).to_le_bytes());
        buf.extend_from_slice(&(record_size as i16).to_le_bytes());
        buf.extend_from_slice(&[0u8; 20]); // reserved

        // Field descriptor: NAME, Character, field_len
        let mut name_bytes = [0u8; 11];
        for (i, b) in b"NAME".iter().enumerate() {
            name_bytes[i] = *b;
        }
        buf.extend_from_slice(&name_bytes);
        buf.push(b'C'); // type
        buf.extend_from_slice(&[0u8; 4]); // reserved
        buf.push(field_len);
        buf.push(0); // decimal count
        buf.extend_from_slice(&[0u8; 14]); // reserved

        // Terminator
        buf.push(0x0D);

        // Records
        for name in names {
            buf.push(0x20); // deletion flag (valid)
            let mut val = name.as_bytes().to_vec();
            val.resize(field_len as usize, b' ');
            buf.extend_from_slice(&val);
        }

        buf
    }

    /// Build a .dbf with NAME (Character) and VALUE (Numeric) fields.
    fn build_dbf_with_numeric(records: &[(&str, &str)], name_len: u8, val_len: u8) -> Vec<u8> {
        let mut buf = Vec::new();

        let num_fields: usize = 2;
        let header_size: u16 = 32 + (num_fields as u16 * 32) + 1;
        let record_size: u16 = 1 + name_len as u16 + val_len as u16;

        // Main header
        buf.push(0x03);
        buf.extend_from_slice(&[24, 1, 1]);
        buf.extend_from_slice(&(records.len() as i32).to_le_bytes());
        buf.extend_from_slice(&(header_size as i16).to_le_bytes());
        buf.extend_from_slice(&(record_size as i16).to_le_bytes());
        buf.extend_from_slice(&[0u8; 20]);

        // Field 1: NAME, Character
        let mut name_bytes = [0u8; 11];
        for (i, b) in b"NAME".iter().enumerate() {
            name_bytes[i] = *b;
        }
        buf.extend_from_slice(&name_bytes);
        buf.push(b'C');
        buf.extend_from_slice(&[0u8; 4]);
        buf.push(name_len);
        buf.push(0);
        buf.extend_from_slice(&[0u8; 14]);

        // Field 2: VALUE, Numeric
        let mut val_name_bytes = [0u8; 11];
        for (i, b) in b"VALUE".iter().enumerate() {
            val_name_bytes[i] = *b;
        }
        buf.extend_from_slice(&val_name_bytes);
        buf.push(b'N');
        buf.extend_from_slice(&[0u8; 4]);
        buf.push(val_len);
        buf.push(2);
        buf.extend_from_slice(&[0u8; 14]);

        // Terminator
        buf.push(0x0D);

        // Records
        for (name, val) in records {
            buf.push(0x20);
            let mut n = name.as_bytes().to_vec();
            n.resize(name_len as usize, b' ');
            buf.extend_from_slice(&n);
            let mut v = vec![b' '; val_len as usize - val.len()];
            v.extend_from_slice(val.as_bytes());
            buf.extend_from_slice(&v);
        }

        buf
    }

    /// Write fixture files to a temp dir and return the .shp path.
    fn write_fixtures(dir: &TempDir, shp_data: &[u8], dbf_data: &[u8]) -> std::path::PathBuf {
        let shp_path = dir.path().join("test.shp");
        let dbf_path = dir.path().join("test.dbf");
        File::create(&shp_path)
            .unwrap()
            .write_all(shp_data)
            .unwrap();
        File::create(&dbf_path)
            .unwrap()
            .write_all(dbf_data)
            .unwrap();
        shp_path
    }

    #[test]
    fn test_open_and_basic_properties() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 2.0), (3.0, 4.0)]);
        let dbf = build_dbf_bytes(&["A", "B"], 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let reader = ShapefileReader::open(&shp_path).unwrap();
        assert_eq!(reader.shape_type(), ShapeType::Point);
        assert_eq!(reader.len(), 2);
        assert!(!reader.is_empty());
        assert!(reader.crs().is_none());
    }

    #[test]
    fn test_open_missing_shp() {
        let result = ShapefileReader::open("/tmp/nonexistent_shapefile_test.shp");
        assert!(matches!(result, Err(ShapefileError::MissingFile { .. })));
    }

    #[test]
    fn test_open_missing_dbf() {
        let dir = TempDir::new().unwrap();
        let shp_path = dir.path().join("test.shp");
        File::create(&shp_path)
            .unwrap()
            .write_all(&build_shp_bytes(&[(1.0, 2.0)]))
            .unwrap();
        // No .dbf file
        let result = ShapefileReader::open(&shp_path);
        assert!(matches!(result, Err(ShapefileError::MissingFile { .. })));
    }

    #[test]
    fn test_records_all() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(10.0, 20.0), (30.0, 40.0), (50.0, 60.0)]);
        let dbf = build_dbf_bytes(&["AA", "BB", "CC"], 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let records = reader.records(None).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].record_number, 1);
        assert_eq!(records[2].record_number, 3);

        // Verify geometry
        let pt = records[0].geometry.as_point().unwrap();
        assert!((pt.x - 10.0).abs() < 1e-10);
        assert!((pt.y - 20.0).abs() < 1e-10);

        // Verify attribute
        assert_eq!(
            records[0].get_attr("NAME"),
            Some(&AttributeValue::Text("AA".to_string()))
        );
    }

    #[test]
    fn test_records_with_limit() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
        let dbf = build_dbf_bytes(&["A", "B", "C"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let records = reader.records(Some(2)).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_iter_records() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 2.0), (3.0, 4.0)]);
        let dbf = build_dbf_bytes(&["X", "Y"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let records: Vec<_> = reader.iter_records().collect();
        assert_eq!(records.len(), 2);
        for r in &records {
            assert!(r.is_ok());
        }
    }

    #[test]
    fn test_bbox() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 2.0), (5.0, 8.0)]);
        let dbf = build_dbf_bytes(&["A", "B"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let reader = ShapefileReader::open(&shp_path).unwrap();
        let bb = reader.bbox();
        assert!((bb.x_min - 1.0).abs() < 1e-10);
        assert!((bb.y_min - 2.0).abs() < 1e-10);
        assert!((bb.x_max - 5.0).abs() < 1e-10);
        assert!((bb.y_max - 8.0).abs() < 1e-10);
    }

    #[test]
    fn test_is_empty() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[]);
        let dbf = build_dbf_bytes(&[], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let reader = ShapefileReader::open(&shp_path).unwrap();
        assert!(reader.is_empty());
        assert_eq!(reader.len(), 0);
    }

    #[test]
    fn test_get_record_by_index() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(10.0, 20.0), (30.0, 40.0)]);
        let dbf = build_dbf_bytes(&["First", "Second"], 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();

        // Get first record
        let rec = reader.get(0).unwrap().unwrap();
        let pt = rec.geometry.as_point().unwrap();
        assert!((pt.x - 10.0).abs() < 1e-10);
        assert_eq!(
            rec.get_attr("NAME"),
            Some(&AttributeValue::Text("First".to_string()))
        );

        // Get second record
        let rec1 = reader.get(1).unwrap().unwrap();
        let pt1 = rec1.geometry.as_point().unwrap();
        assert!((pt1.x - 30.0).abs() < 1e-10);

        // Out of bounds
        let none = reader.get(5).unwrap();
        assert!(none.is_none());
    }

    #[test]
    fn test_filter_by_attribute() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
        let dbf = build_dbf_bytes(&["Tokyo", "Osaka", "Tokyo"], 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let filtered = reader
            .filter_by_attribute("NAME", &AttributeValue::Text("Tokyo".to_string()))
            .unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_attribute_field_not_found() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0)]);
        let dbf = build_dbf_bytes(&["A"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let result = reader.filter_by_attribute("MISSING", &AttributeValue::Text("X".to_string()));
        assert!(matches!(result, Err(ShapefileError::FieldNotFound(_))));
    }

    #[test]
    fn test_filter_by_attribute_in() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
        let dbf = build_dbf_bytes(&["A", "B", "C"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let filtered = reader
            .filter_by_attribute_in(
                "NAME",
                &[
                    AttributeValue::Text("A".to_string()),
                    AttributeValue::Text("C".to_string()),
                ],
            )
            .unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_attribute_starts_with() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
        let dbf = build_dbf_bytes(&["Tokyo", "Toyama", "Osaka"], 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let filtered = reader
            .filter_by_attribute_starts_with("NAME", "To")
            .unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_bbox() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0), (50.0, 50.0), (100.0, 100.0)]);
        let dbf = build_dbf_bytes(&["A", "B", "C"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let bbox = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 60.0,
            y_max: 60.0,
        };
        let filtered = reader.filter_by_bbox(&bbox).unwrap();
        assert_eq!(filtered.len(), 2); // (1,1) and (50,50) are inside
    }

    #[test]
    fn test_describe_numeric_field() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
        let dbf = build_dbf_with_numeric(&[("A", "10.00"), ("B", "20.00"), ("C", "30.00")], 10, 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let stats = reader.describe("VALUE").unwrap();
        assert_eq!(stats.count, 3);
        assert!((stats.min - 10.0).abs() < 1e-10);
        assert!((stats.max - 30.0).abs() < 1e-10);
        assert!((stats.mean - 20.0).abs() < 1e-10);
        assert!((stats.median - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_describe_field_not_found() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0)]);
        let dbf = build_dbf_bytes(&["A"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let result = reader.describe("NONEXISTENT");
        assert!(matches!(result, Err(ShapefileError::FieldNotFound(_))));
    }

    #[test]
    fn test_describe_non_numeric_field() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0)]);
        let dbf = build_dbf_bytes(&["A"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let result = reader.describe("NAME");
        assert!(matches!(
            result,
            Err(ShapefileError::DescribeOnNonNumericField { .. })
        ));
    }

    #[test]
    fn test_open_with_prj() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(139.0, 35.0)]);
        let dbf = build_dbf_bytes(&["Tokyo"], 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        // Write a .prj file
        let prj_path = dir.path().join("test.prj");
        File::create(&prj_path)
            .unwrap()
            .write_all(b"GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]]")
            .unwrap();

        let reader = ShapefileReader::open(&shp_path).unwrap();
        assert!(reader.crs().is_some());
    }

    #[test]
    fn test_open_with_cpg() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 2.0)]);
        let dbf = build_dbf_bytes(&["Hello"], 10);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        // Write a .cpg file
        let cpg_path = dir.path().join("test.cpg");
        File::create(&cpg_path)
            .unwrap()
            .write_all(b"UTF-8")
            .unwrap();

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        let records = reader.records(None).unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_validate_field_exists_error() {
        let dir = TempDir::new().unwrap();
        let shp = build_shp_bytes(&[(1.0, 1.0)]);
        let dbf = build_dbf_bytes(&["A"], 5);
        let shp_path = write_fixtures(&dir, &shp, &dbf);

        let mut reader = ShapefileReader::open(&shp_path).unwrap();
        // filter_by_attribute_in calls validate_field_exists internally
        let result = reader.filter_by_attribute_in("NOPE", &[]);
        assert!(matches!(result, Err(ShapefileError::FieldNotFound(_))));
    }
}

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
