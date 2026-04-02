use std::io::{Read, Seek};

use crate::error::ShapefileError;
use crate::io::binary_reader::BinaryReader;
use crate::models::bbox::BoundingBox;
use crate::models::geometry::{Geometry, MultiPoint, Point, Polygon, Polyline, Ring};

/// ESRI shape type codes as defined in the Shapefile specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ShapeType {
    /// Null shape (no geometry).
    Null = 0,
    /// Single 2D point.
    Point = 1,
    /// Ordered set of 2D vertices forming line segments.
    Polyline = 3,
    /// One or more closed rings forming a polygon.
    Polygon = 5,
    /// Set of unconnected 2D points.
    MultiPoint = 8,
    /// Single 3D point with Z and optional M values.
    PointZ = 11,
    /// Polyline with Z coordinates.
    PolylineZ = 13,
    /// Polygon with Z coordinates.
    PolygonZ = 15,
    /// Single 2D point with a measure (M) value.
    PointM = 21,
    /// Polyline with measure (M) values.
    PolylineM = 23,
    /// Polygon with measure (M) values.
    PolygonM = 25,
    /// Complex 3D surface patches.
    MultiPatch = 31,
}

impl TryFrom<i32> for ShapeType {
    type Error = ShapefileError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ShapeType::Null),
            1 => Ok(ShapeType::Point),
            3 => Ok(ShapeType::Polyline),
            5 => Ok(ShapeType::Polygon),
            8 => Ok(ShapeType::MultiPoint),
            11 => Ok(ShapeType::PointZ),
            13 => Ok(ShapeType::PolylineZ),
            15 => Ok(ShapeType::PolygonZ),
            21 => Ok(ShapeType::PointM),
            23 => Ok(ShapeType::PolylineM),
            25 => Ok(ShapeType::PolygonM),
            31 => Ok(ShapeType::MultiPatch),
            n => Err(ShapefileError::UnsupportedShapeType(n)),
        }
    }
}

pub(crate) struct ShpHeader {
    pub(crate) shape_type: ShapeType,
    pub(crate) file_length: i32,
    pub(crate) bbox: BoundingBox,
}

pub(crate) struct ShxIndex {
    /// (offset_bytes, content_length_bytes) list
    pub(crate) entries: Vec<(u64, u32)>,
}

pub(crate) struct ShpParser<R: Read + Seek> {
    reader: BinaryReader<R>,
    shx: Option<ShxIndex>,
}

pub(crate) struct ShpRecordIter<'a, R: Read + Seek> {
    parser: &'a mut ShpParser<R>,
}

impl<'a, R: Read + Seek> Iterator for ShpRecordIter<'a, R> {
    type Item = Result<Geometry, ShapefileError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.parser.reader.is_eof() {
            Ok(true) => None,
            Ok(false) => Some(self.parser.parse_record()),
            Err(e) => Some(Err(e)),
        }
    }
}

impl<R: Read + Seek> ShpParser<R> {
    pub fn new(shp: R, shx: Option<R>) -> Result<Self, ShapefileError> {
        let shx_index = if let Some(shx_reader) = shx {
            let mut shx_br = BinaryReader::new(shx_reader);
            // Read SHX header (100 bytes, same format as SHP)
            let file_code = shx_br.read_i32_be()?;
            if file_code != 9994 {
                return Err(ShapefileError::InvalidFileCode { actual: file_code });
            }
            let _unused = shx_br.read_bytes(20)?;
            let file_length = shx_br.read_i32_be()?; // in 16-bit words
            let _version = shx_br.read_i32_le()?;
            let _shape_type = shx_br.read_i32_le()?;
            let _bbox = shx_br.read_bytes(64)?; // skip bbox + z/m ranges

            // Number of index entries: (file_length_words * 2 - 100) / 8
            let file_length_bytes = file_length as u64 * 2;
            let num_entries = (file_length_bytes - 100) / 8;

            let mut entries = Vec::with_capacity(num_entries as usize);
            for _ in 0..num_entries {
                let offset_words = shx_br.read_i32_be()? as u64;
                let content_length_words = shx_br.read_i32_be()? as u32;
                entries.push((offset_words * 2, content_length_words * 2));
            }
            Some(ShxIndex { entries })
        } else {
            None
        };

        Ok(ShpParser {
            reader: BinaryReader::new(shp),
            shx: shx_index,
        })
    }

    pub fn parse_header(&mut self) -> Result<ShpHeader, ShapefileError> {
        let file_code = self.reader.read_i32_be()?;
        if file_code != 9994 {
            return Err(ShapefileError::InvalidFileCode { actual: file_code });
        }
        let _unused = self.reader.read_bytes(20)?;
        let file_length = self.reader.read_i32_be()?;
        let version = self.reader.read_i32_le()?;
        if version != 1000 {
            return Err(ShapefileError::InvalidVersion { actual: version });
        }
        let shape_type_code = self.reader.read_i32_le()?;
        let shape_type = ShapeType::try_from(shape_type_code)?;

        let x_min = self.reader.read_f64_le()?;
        let y_min = self.reader.read_f64_le()?;
        let x_max = self.reader.read_f64_le()?;
        let y_max = self.reader.read_f64_le()?;

        // Skip remaining bbox fields (z_min, z_max, m_min, m_max)
        let _z_min = self.reader.read_f64_le()?;
        let _z_max = self.reader.read_f64_le()?;
        let _m_min = self.reader.read_f64_le()?;
        let _m_max = self.reader.read_f64_le()?;

        Ok(ShpHeader {
            shape_type,
            file_length,
            bbox: BoundingBox {
                x_min,
                y_min,
                x_max,
                y_max,
            },
        })
    }

    pub fn parse_record(&mut self) -> Result<Geometry, ShapefileError> {
        // Record header: record_number (i32 BE), content_length (i32 BE, in 16-bit words)
        let _record_number = self.reader.read_i32_be()?;
        let _content_length = self.reader.read_i32_be()?;

        // Shape type (i32 LE)
        let shape_type_code = self.reader.read_i32_le()?;

        match shape_type_code {
            0 => Ok(Geometry::Null),
            1 => {
                // Point: x (f64 LE), y (f64 LE)
                let x = self.reader.read_f64_le()?;
                let y = self.reader.read_f64_le()?;
                Ok(Geometry::Point(Point { x, y }))
            }
            3 => {
                // Polyline
                self.parse_polyline_content()
            }
            5 => {
                // Polygon
                self.parse_polygon_content()
            }
            8 => {
                // MultiPoint
                self.parse_multipoint_content()
            }
            n => Err(ShapefileError::UnsupportedShapeType(n)),
        }
    }

    fn parse_polyline_content(&mut self) -> Result<Geometry, ShapefileError> {
        // Skip bbox (4x f64 LE)
        let _x_min = self.reader.read_f64_le()?;
        let _y_min = self.reader.read_f64_le()?;
        let _x_max = self.reader.read_f64_le()?;
        let _y_max = self.reader.read_f64_le()?;

        let num_parts = self.reader.read_i32_le()? as usize;
        let num_points = self.reader.read_i32_le()? as usize;

        // Part indices
        let part_starts: Vec<usize> = (0..num_parts)
            .map(|_| self.reader.read_i32_le().map(|v| v as usize))
            .collect::<Result<_, _>>()?;

        // Points
        let points: Vec<Point> = (0..num_points)
            .map(|_| {
                Ok(Point {
                    x: self.reader.read_f64_le()?,
                    y: self.reader.read_f64_le()?,
                })
            })
            .collect::<Result<_, ShapefileError>>()?;

        // Split points into parts
        let parts: Vec<Vec<Point>> = part_starts
            .windows(2)
            .chain(std::iter::once(
                &[*part_starts.last().unwrap(), num_points][..],
            ))
            .map(|w| points[w[0]..w[1]].to_vec())
            .collect();

        Ok(Geometry::Polyline(Polyline { parts }))
    }

    fn parse_polygon_content(&mut self) -> Result<Geometry, ShapefileError> {
        // Skip bbox (4x f64 LE)
        let _x_min = self.reader.read_f64_le()?;
        let _y_min = self.reader.read_f64_le()?;
        let _x_max = self.reader.read_f64_le()?;
        let _y_max = self.reader.read_f64_le()?;

        let num_parts = self.reader.read_i32_le()? as usize;
        let num_points = self.reader.read_i32_le()? as usize;

        // Part indices
        let part_starts: Vec<usize> = (0..num_parts)
            .map(|_| self.reader.read_i32_le().map(|v| v as usize))
            .collect::<Result<_, _>>()?;

        // Points
        let points: Vec<Point> = (0..num_points)
            .map(|_| {
                Ok(Point {
                    x: self.reader.read_f64_le()?,
                    y: self.reader.read_f64_le()?,
                })
            })
            .collect::<Result<_, ShapefileError>>()?;

        // Split points into rings
        let rings: Vec<Ring> = part_starts
            .windows(2)
            .chain(std::iter::once(
                &[*part_starts.last().unwrap(), num_points][..],
            ))
            .map(|w| Ring {
                points: points[w[0]..w[1]].to_vec(),
            })
            .collect();

        Ok(Geometry::Polygon(Polygon { rings }))
    }

    fn parse_multipoint_content(&mut self) -> Result<Geometry, ShapefileError> {
        // Skip bbox (4x f64 LE)
        let _x_min = self.reader.read_f64_le()?;
        let _y_min = self.reader.read_f64_le()?;
        let _x_max = self.reader.read_f64_le()?;
        let _y_max = self.reader.read_f64_le()?;

        let num_points = self.reader.read_i32_le()? as usize;

        let points: Vec<Point> = (0..num_points)
            .map(|_| {
                Ok(Point {
                    x: self.reader.read_f64_le()?,
                    y: self.reader.read_f64_le()?,
                })
            })
            .collect::<Result<_, ShapefileError>>()?;

        Ok(Geometry::MultiPoint(MultiPoint { points }))
    }

    /// Seek back to the start of record data (byte offset 100, right after the header).
    /// Call this before iter_records if you need to re-iterate.
    pub fn seek_to_records(&mut self) -> Result<(), ShapefileError> {
        self.reader.seek_from_start(100)?;
        Ok(())
    }

    /// Whether this parser has a SHX index for random access.
    pub fn has_shx(&self) -> bool {
        self.shx.is_some()
    }

    pub fn iter_records(&mut self) -> ShpRecordIter<'_, R> {
        ShpRecordIter { parser: self }
    }

    pub fn read_at(&mut self, index: usize) -> Result<Option<Geometry>, ShapefileError> {
        match &self.shx {
            Some(shx) => {
                if index >= shx.entries.len() {
                    return Ok(None);
                }
                let (offset, _content_length) = shx.entries[index];
                self.reader.seek_from_start(offset)?;
                Ok(Some(self.parse_record()?))
            }
            None => Err(ShapefileError::CorruptedFile {
                reason: "SHX index not available for random access".to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal valid .shp header
    fn minimal_shp_header(shape_type: i32) -> Vec<u8> {
        let mut buf = Vec::with_capacity(100);
        buf.extend_from_slice(&9994_i32.to_be_bytes());
        buf.extend_from_slice(&[0u8; 20]);
        buf.extend_from_slice(&50_i32.to_be_bytes());
        buf.extend_from_slice(&1000_i32.to_le_bytes());
        buf.extend_from_slice(&shape_type.to_le_bytes());
        buf.extend_from_slice(&[0u8; 64]); // bbox + z/m ranges
        buf
    }

    /// Build a point record: record_header + shape_type(1) + x + y
    fn build_point_record(record_number: i32, x: f64, y: f64) -> Vec<u8> {
        let mut buf = Vec::new();
        // Record header
        buf.extend_from_slice(&record_number.to_be_bytes());
        // content_length in 16-bit words: shape_type(4) + x(8) + y(8) = 20 bytes = 10 words
        buf.extend_from_slice(&10_i32.to_be_bytes());
        // Shape type
        buf.extend_from_slice(&1_i32.to_le_bytes());
        // x, y
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf
    }

    /// Build a null record
    fn build_null_record(record_number: i32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&record_number.to_be_bytes());
        // content_length: shape_type(4) = 4 bytes = 2 words
        buf.extend_from_slice(&2_i32.to_be_bytes());
        buf.extend_from_slice(&0_i32.to_le_bytes());
        buf
    }

    /// Build a polyline record with given parts and points
    fn build_polyline_record(
        record_number: i32,
        part_starts: &[i32],
        points: &[(f64, f64)],
    ) -> Vec<u8> {
        let num_parts = part_starts.len();
        let num_points = points.len();
        // content: shape_type(4) + bbox(32) + num_parts(4) + num_points(4)
        //        + parts(num_parts*4) + points(num_points*16)
        let content_bytes = 4 + 32 + 4 + 4 + num_parts * 4 + num_points * 16;
        let content_words = content_bytes / 2;

        let mut buf = Vec::new();
        buf.extend_from_slice(&record_number.to_be_bytes());
        buf.extend_from_slice(&(content_words as i32).to_be_bytes());
        // shape type = 3 (Polyline)
        buf.extend_from_slice(&3_i32.to_le_bytes());
        // bbox (zeros)
        buf.extend_from_slice(&[0u8; 32]);
        buf.extend_from_slice(&(num_parts as i32).to_le_bytes());
        buf.extend_from_slice(&(num_points as i32).to_le_bytes());
        for &ps in part_starts {
            buf.extend_from_slice(&ps.to_le_bytes());
        }
        for &(x, y) in points {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
        }
        buf
    }

    /// Build a polygon record with given parts and points
    fn build_polygon_record(
        record_number: i32,
        part_starts: &[i32],
        points: &[(f64, f64)],
    ) -> Vec<u8> {
        let num_parts = part_starts.len();
        let num_points = points.len();
        let content_bytes = 4 + 32 + 4 + 4 + num_parts * 4 + num_points * 16;
        let content_words = content_bytes / 2;

        let mut buf = Vec::new();
        buf.extend_from_slice(&record_number.to_be_bytes());
        buf.extend_from_slice(&(content_words as i32).to_be_bytes());
        // shape type = 5 (Polygon)
        buf.extend_from_slice(&5_i32.to_le_bytes());
        // bbox (zeros)
        buf.extend_from_slice(&[0u8; 32]);
        buf.extend_from_slice(&(num_parts as i32).to_le_bytes());
        buf.extend_from_slice(&(num_points as i32).to_le_bytes());
        for &ps in part_starts {
            buf.extend_from_slice(&ps.to_le_bytes());
        }
        for &(x, y) in points {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
        }
        buf
    }

    #[test]
    fn test_parse_header_file_code() {
        let data = minimal_shp_header(3);
        let mut parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        let header = parser.parse_header().unwrap();
        // If we got here, file code was valid (9994)
        assert_eq!(header.shape_type, ShapeType::Polyline);
    }

    #[test]
    fn test_parse_header_shape_type() {
        let data = minimal_shp_header(1);
        let mut parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        let header = parser.parse_header().unwrap();
        assert_eq!(header.shape_type, ShapeType::Point);
    }

    #[test]
    fn test_parse_header_invalid_file_code() {
        let mut data = minimal_shp_header(3);
        // Overwrite file code with invalid value
        data[0..4].copy_from_slice(&1234_i32.to_be_bytes());
        let mut parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        let result = parser.parse_header();
        assert!(matches!(
            result,
            Err(ShapefileError::InvalidFileCode { actual: 1234 })
        ));
    }

    #[test]
    fn test_parse_point_record() {
        let record = build_point_record(1, 139.6917, 35.6895);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let pt = geom.as_point().unwrap();
        assert!((pt.x - 139.6917).abs() < 1e-10);
        assert!((pt.y - 35.6895).abs() < 1e-10);
    }

    #[test]
    fn test_parse_polyline_record_1part_2pt() {
        let points = vec![(0.0, 0.0), (1.0, 1.0)];
        let record = build_polyline_record(1, &[0], &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let polyline = geom.as_polyline().unwrap();
        assert_eq!(polyline.parts.len(), 1);
        assert_eq!(polyline.parts[0].len(), 2);
        assert!((polyline.parts[0][0].x - 0.0).abs() < 1e-10);
        assert!((polyline.parts[0][0].y - 0.0).abs() < 1e-10);
        assert!((polyline.parts[0][1].x - 1.0).abs() < 1e-10);
        assert!((polyline.parts[0][1].y - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_polyline_num_parts() {
        let points = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        let record = build_polyline_record(1, &[0, 2], &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let polyline = geom.as_polyline().unwrap();
        assert_eq!(polyline.num_parts(), 2);
    }

    #[test]
    fn test_parse_polyline_num_points() {
        let points = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)];
        let record = build_polyline_record(1, &[0, 2], &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let polyline = geom.as_polyline().unwrap();
        assert_eq!(polyline.num_points(), 5);
    }

    #[test]
    fn test_parse_polyline_length() {
        // Single part: (0,0) -> (3,4), expected length = 5.0
        let points = vec![(0.0, 0.0), (3.0, 4.0)];
        let record = build_polyline_record(1, &[0], &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let polyline = geom.as_polyline().unwrap();
        assert!((polyline.length() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_polygon_record() {
        // Single ring: square (0,0) -> (4,0) -> (4,4) -> (0,4) -> (0,0)
        let points = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0), (0.0, 0.0)];
        let record = build_polygon_record(1, &[0], &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let polygon = geom.as_polygon().unwrap();
        assert_eq!(polygon.rings.len(), 1);
        assert_eq!(polygon.rings[0].points.len(), 5);
        assert!((polygon.rings[0].points[0].x - 0.0).abs() < 1e-10);
        assert!((polygon.rings[0].points[1].x - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_null_record() {
        let record = build_null_record(1);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        assert!(geom.is_null());
    }

    #[test]
    fn test_unknown_shape_type_error() {
        let result = ShapeType::try_from(999);
        assert!(matches!(
            result,
            Err(ShapefileError::UnsupportedShapeType(999))
        ));
    }

    #[test]
    fn test_iter_records_count() {
        // Build a minimal .shp with header + 3 point records
        let mut data = minimal_shp_header(1);
        data.extend_from_slice(&build_point_record(1, 1.0, 2.0));
        data.extend_from_slice(&build_point_record(2, 3.0, 4.0));
        data.extend_from_slice(&build_point_record(3, 5.0, 6.0));

        let mut parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        // Skip the header first
        parser.parse_header().unwrap();

        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 3);
        for r in &records {
            assert!(r.is_ok());
        }
    }

    /// Build a multipoint record
    fn build_multipoint_record(record_number: i32, points: &[(f64, f64)]) -> Vec<u8> {
        let num_points = points.len();
        // content: shape_type(4) + bbox(32) + num_points(4) + points(num_points*16)
        let content_bytes = 4 + 32 + 4 + num_points * 16;
        let content_words = content_bytes / 2;

        let mut buf = Vec::new();
        buf.extend_from_slice(&record_number.to_be_bytes());
        buf.extend_from_slice(&(content_words as i32).to_be_bytes());
        // shape type = 8 (MultiPoint)
        buf.extend_from_slice(&8_i32.to_le_bytes());
        // bbox (zeros)
        buf.extend_from_slice(&[0u8; 32]);
        buf.extend_from_slice(&(num_points as i32).to_le_bytes());
        for &(x, y) in points {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
        }
        buf
    }

    #[test]
    fn test_parse_multipoint_record() {
        let points = vec![(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)];
        let record = build_multipoint_record(1, &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        match &geom {
            Geometry::MultiPoint(mp) => {
                assert_eq!(mp.points.len(), 3);
                assert!((mp.points[0].x - 1.0).abs() < 1e-10);
                assert!((mp.points[2].y - 6.0).abs() < 1e-10);
            }
            _ => panic!("expected MultiPoint"),
        }
    }

    #[test]
    fn test_shape_type_try_from_all_valid() {
        assert_eq!(ShapeType::try_from(0).unwrap(), ShapeType::Null);
        assert_eq!(ShapeType::try_from(1).unwrap(), ShapeType::Point);
        assert_eq!(ShapeType::try_from(3).unwrap(), ShapeType::Polyline);
        assert_eq!(ShapeType::try_from(5).unwrap(), ShapeType::Polygon);
        assert_eq!(ShapeType::try_from(8).unwrap(), ShapeType::MultiPoint);
        assert_eq!(ShapeType::try_from(11).unwrap(), ShapeType::PointZ);
        assert_eq!(ShapeType::try_from(13).unwrap(), ShapeType::PolylineZ);
        assert_eq!(ShapeType::try_from(15).unwrap(), ShapeType::PolygonZ);
        assert_eq!(ShapeType::try_from(21).unwrap(), ShapeType::PointM);
        assert_eq!(ShapeType::try_from(23).unwrap(), ShapeType::PolylineM);
        assert_eq!(ShapeType::try_from(25).unwrap(), ShapeType::PolygonM);
        assert_eq!(ShapeType::try_from(31).unwrap(), ShapeType::MultiPatch);
    }

    #[test]
    fn test_shape_type_try_from_invalid() {
        assert!(ShapeType::try_from(2).is_err());
        assert!(ShapeType::try_from(-1).is_err());
        assert!(ShapeType::try_from(100).is_err());
    }

    #[test]
    fn test_has_shx_false() {
        let data = minimal_shp_header(1);
        let parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        assert!(!parser.has_shx());
    }

    #[test]
    fn test_seek_to_records() {
        let mut data = minimal_shp_header(1);
        data.extend_from_slice(&build_point_record(1, 10.0, 20.0));

        let mut parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        parser.parse_header().unwrap();

        // Read the record
        let geom1 = parser.parse_record().unwrap();
        let pt1 = geom1.as_point().unwrap();
        assert!((pt1.x - 10.0).abs() < 1e-10);

        // Seek back and re-read
        parser.seek_to_records().unwrap();
        let geom2 = parser.parse_record().unwrap();
        let pt2 = geom2.as_point().unwrap();
        assert!((pt2.x - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_read_at_without_shx_returns_error() {
        let data = minimal_shp_header(1);
        let mut parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        let result = parser.read_at(0);
        assert!(matches!(result, Err(ShapefileError::CorruptedFile { .. })));
    }

    #[test]
    fn test_parse_header_invalid_version() {
        let mut data = Vec::with_capacity(100);
        data.extend_from_slice(&9994_i32.to_be_bytes());
        data.extend_from_slice(&[0u8; 20]);
        data.extend_from_slice(&50_i32.to_be_bytes());
        // Invalid version (not 1000)
        data.extend_from_slice(&999_i32.to_le_bytes());
        data.extend_from_slice(&1_i32.to_le_bytes()); // shape type
        data.extend_from_slice(&[0u8; 64]); // bbox + z/m

        let mut parser = ShpParser::new(Cursor::new(data), None::<Cursor<Vec<u8>>>).unwrap();
        let result = parser.parse_header();
        assert!(matches!(
            result,
            Err(ShapefileError::InvalidVersion { actual: 999 })
        ));
    }

    #[test]
    fn test_parse_unsupported_record_shape_type() {
        // Build a record with shape type 31 (MultiPatch) which parse_record doesn't handle
        let mut buf = Vec::new();
        buf.extend_from_slice(&1_i32.to_be_bytes()); // record number
        buf.extend_from_slice(&2_i32.to_be_bytes()); // content length (2 words = 4 bytes)
        buf.extend_from_slice(&31_i32.to_le_bytes()); // shape type = MultiPatch

        let mut parser = ShpParser::new(Cursor::new(buf), None::<Cursor<Vec<u8>>>).unwrap();
        let result = parser.parse_record();
        assert!(matches!(
            result,
            Err(ShapefileError::UnsupportedShapeType(31))
        ));
    }

    #[test]
    fn test_parse_polygon_with_hole() {
        // Exterior ring + hole
        let points = vec![
            // Exterior: (0,0) -> (10,0) -> (10,10) -> (0,10) -> (0,0)
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
            // Hole: (2,2) -> (3,2) -> (3,3) -> (2,3) -> (2,2)
            (2.0, 2.0),
            (3.0, 2.0),
            (3.0, 3.0),
            (2.0, 3.0),
            (2.0, 2.0),
        ];
        let record = build_polygon_record(1, &[0, 5], &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let polygon = geom.as_polygon().unwrap();
        assert_eq!(polygon.rings.len(), 2);
        assert_eq!(polygon.exterior().points.len(), 5);
        assert_eq!(polygon.holes().len(), 1);
        assert_eq!(polygon.holes()[0].points.len(), 5);
    }

    #[test]
    fn test_parse_polyline_multipart() {
        let points = vec![
            (0.0, 0.0),
            (1.0, 1.0),
            (2.0, 2.0),
            (10.0, 10.0),
            (11.0, 11.0),
        ];
        let record = build_polyline_record(1, &[0, 3], &points);
        let mut parser = ShpParser::new(Cursor::new(record), None::<Cursor<Vec<u8>>>).unwrap();
        let geom = parser.parse_record().unwrap();
        let polyline = geom.as_polyline().unwrap();
        assert_eq!(polyline.parts.len(), 2);
        assert_eq!(polyline.parts[0].len(), 3);
        assert_eq!(polyline.parts[1].len(), 2);
    }

    /// Build a minimal valid .shx file for testing random access
    fn build_shx_for_points(num_records: usize, shp_header_size: u64) -> Vec<u8> {
        // SHX header = 100 bytes, each entry = 8 bytes
        let file_length_bytes = 100 + num_records * 8;
        let file_length_words = file_length_bytes / 2;

        let mut buf = Vec::with_capacity(file_length_bytes);
        // File code
        buf.extend_from_slice(&9994_i32.to_be_bytes());
        // Unused (20 bytes)
        buf.extend_from_slice(&[0u8; 20]);
        // File length in 16-bit words
        buf.extend_from_slice(&(file_length_words as i32).to_be_bytes());
        // Version
        buf.extend_from_slice(&1000_i32.to_le_bytes());
        // Shape type
        buf.extend_from_slice(&1_i32.to_le_bytes());
        // bbox + z/m ranges (64 bytes)
        buf.extend_from_slice(&[0u8; 64]);

        // Each point record is 28 bytes (8 header + 20 content)
        let record_size: u64 = 28;
        for i in 0..num_records {
            let offset_bytes = shp_header_size + (i as u64) * record_size;
            let offset_words = offset_bytes / 2;
            let content_length_words = 10_u32; // 20 bytes / 2
            buf.extend_from_slice(&(offset_words as i32).to_be_bytes());
            buf.extend_from_slice(&(content_length_words as i32).to_be_bytes());
        }
        buf
    }

    #[test]
    fn test_has_shx_true() {
        let shp_data = minimal_shp_header(1);
        let shx_data = build_shx_for_points(0, 100);
        let parser = ShpParser::new(Cursor::new(shp_data), Some(Cursor::new(shx_data))).unwrap();
        assert!(parser.has_shx());
    }

    #[test]
    fn test_read_at_with_shx() {
        let mut shp_data = minimal_shp_header(1);
        shp_data.extend_from_slice(&build_point_record(1, 100.0, 200.0));
        shp_data.extend_from_slice(&build_point_record(2, 300.0, 400.0));

        let shx_data = build_shx_for_points(2, 100);

        let mut parser =
            ShpParser::new(Cursor::new(shp_data), Some(Cursor::new(shx_data))).unwrap();

        // Read second record (index 1)
        let geom = parser.read_at(1).unwrap().unwrap();
        let pt = geom.as_point().unwrap();
        assert!((pt.x - 300.0).abs() < 1e-10);
        assert!((pt.y - 400.0).abs() < 1e-10);

        // Read first record (index 0)
        let geom0 = parser.read_at(0).unwrap().unwrap();
        let pt0 = geom0.as_point().unwrap();
        assert!((pt0.x - 100.0).abs() < 1e-10);
    }

    #[test]
    fn test_read_at_out_of_bounds() {
        let shp_data = minimal_shp_header(1);
        let shx_data = build_shx_for_points(1, 100);

        let mut parser =
            ShpParser::new(Cursor::new(shp_data), Some(Cursor::new(shx_data))).unwrap();

        let result = parser.read_at(5).unwrap();
        assert!(result.is_none());
    }
}
