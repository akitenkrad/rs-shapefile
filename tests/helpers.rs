use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Minimal valid .shp header per spec
pub fn minimal_shp_header(shape_type: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(100);
    buf.extend_from_slice(&9994_i32.to_be_bytes());
    buf.extend_from_slice(&[0u8; 20]);
    buf.extend_from_slice(&50_i32.to_be_bytes());
    buf.extend_from_slice(&1000_i32.to_le_bytes());
    buf.extend_from_slice(&shape_type.to_le_bytes());
    buf.extend_from_slice(&[0u8; 64]);
    buf
}

/// Polyline record bytes (1 part, N points)
/// record header(8) + shape_type(4) + bbox(32) + num_parts(4)
/// + num_points(4) + parts(4) + points(16*n) bytes total
pub fn polyline_record_bytes(record_num: i32, pts: &[(f64, f64)]) -> Vec<u8> {
    let n = pts.len();
    // content_len (words) = (4 + 32 + 4 + 4 + 4 + 16*n) / 2
    let content_bytes = 4 + 32 + 4 + 4 + 4 + 16 * n;
    let content_words = content_bytes / 2;

    let mut buf = Vec::new();
    buf.extend_from_slice(&record_num.to_be_bytes());
    buf.extend_from_slice(&(content_words as i32).to_be_bytes());

    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();

    buf.extend_from_slice(&3_i32.to_le_bytes()); // shape type
    buf.extend_from_slice(
        &xs.iter()
            .cloned()
            .fold(f64::INFINITY, f64::min)
            .to_le_bytes(),
    );
    buf.extend_from_slice(
        &ys.iter()
            .cloned()
            .fold(f64::INFINITY, f64::min)
            .to_le_bytes(),
    );
    buf.extend_from_slice(
        &xs.iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
            .to_le_bytes(),
    );
    buf.extend_from_slice(
        &ys.iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
            .to_le_bytes(),
    );
    buf.extend_from_slice(&1_i32.to_le_bytes()); // num_parts
    buf.extend_from_slice(&(n as i32).to_le_bytes()); // num_points
    buf.extend_from_slice(&0_i32.to_le_bytes()); // parts[0]
    for (x, y) in pts {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
    }
    buf
}

/// Point record bytes
pub fn point_record_bytes(record_num: i32, x: f64, y: f64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&record_num.to_be_bytes());
    // content_length in 16-bit words: shape_type(4) + x(8) + y(8) = 20 bytes = 10 words
    buf.extend_from_slice(&10_i32.to_be_bytes());
    buf.extend_from_slice(&1_i32.to_le_bytes()); // shape type = Point
    buf.extend_from_slice(&x.to_le_bytes());
    buf.extend_from_slice(&y.to_le_bytes());
    buf
}

/// Full .shp file (header + 1 polyline record) as Cursor
pub fn polyline_shp_cursor(pts: &[(f64, f64)]) -> Cursor<Vec<u8>> {
    let mut buf = minimal_shp_header(3);
    buf.extend(polyline_record_bytes(1, pts));
    Cursor::new(buf)
}

/// Build a .shp header with correct file_length and bbox
pub fn shp_header_with_bbox(
    shape_type: i32,
    file_length_words: i32,
    x_min: f64,
    y_min: f64,
    x_max: f64,
    y_max: f64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(100);
    buf.extend_from_slice(&9994_i32.to_be_bytes()); // file code
    buf.extend_from_slice(&[0u8; 20]); // unused
    buf.extend_from_slice(&file_length_words.to_be_bytes()); // file length in 16-bit words
    buf.extend_from_slice(&1000_i32.to_le_bytes()); // version
    buf.extend_from_slice(&shape_type.to_le_bytes()); // shape type
    buf.extend_from_slice(&x_min.to_le_bytes());
    buf.extend_from_slice(&y_min.to_le_bytes());
    buf.extend_from_slice(&x_max.to_le_bytes());
    buf.extend_from_slice(&y_max.to_le_bytes());
    // z_min, z_max, m_min, m_max
    buf.extend_from_slice(&[0u8; 32]);
    buf
}

/// Build a complete .shp file with a proper header + records
pub fn build_shp_file(shape_type: i32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut data = Vec::new();
    // Reserve space for header (100 bytes)
    data.extend_from_slice(&[0u8; 100]);

    // Append records
    for rec in records {
        data.extend_from_slice(rec);
    }

    // Calculate file length in 16-bit words
    let file_length_words = (data.len() / 2) as i32;

    // Now write the header at the beginning
    let header = shp_header_with_bbox(shape_type, file_length_words, 0.0, 0.0, 100.0, 100.0);
    data[..100].copy_from_slice(&header);

    data
}

/// Build a minimal valid .dbf file
/// `fields`: slice of (name, type_byte, length, decimal_count)
/// `records`: slice of (deletion_flag_byte, field_values)
pub fn build_dbf_file(fields: &[(&str, u8, u8, u8)], records: &[(u8, &[Vec<u8>])]) -> Vec<u8> {
    let mut buf = Vec::new();

    let num_fields = fields.len();
    let header_size: u16 = 32 + (num_fields as u16 * 32) + 1;
    let record_size: u16 = 1 + fields.iter().map(|f| f.2 as u16).sum::<u16>();

    // Main header (32 bytes)
    buf.push(0x03); // version
    buf.extend_from_slice(&[24, 1, 1]); // date
    buf.extend_from_slice(&(records.len() as i32).to_le_bytes());
    buf.extend_from_slice(&(header_size as i16).to_le_bytes());
    buf.extend_from_slice(&(record_size as i16).to_le_bytes());
    buf.extend_from_slice(&[0u8; 20]); // reserved

    // Field descriptors
    for (name, type_byte, length, decimal_count) in fields {
        let mut name_bytes = [0u8; 11];
        for (i, b) in name.as_bytes().iter().enumerate().take(11) {
            name_bytes[i] = *b;
        }
        buf.extend_from_slice(&name_bytes);
        buf.push(*type_byte);
        buf.extend_from_slice(&[0u8; 4]); // reserved
        buf.push(*length);
        buf.push(*decimal_count);
        buf.extend_from_slice(&[0u8; 14]); // reserved
    }

    buf.push(0x0D); // terminator

    // Records
    for (flag, field_values) in records {
        buf.push(*flag);
        for val in *field_values {
            buf.extend_from_slice(val);
        }
    }

    buf
}

/// Build .shx file from .shp record offsets
pub fn build_shx_file(shape_type: i32, record_offsets: &[(u64, u32)]) -> Vec<u8> {
    let num_entries = record_offsets.len();
    let file_length_bytes = 100 + num_entries * 8;
    let file_length_words = (file_length_bytes / 2) as i32;

    let mut buf = Vec::with_capacity(file_length_bytes);
    buf.extend_from_slice(&9994_i32.to_be_bytes()); // file code
    buf.extend_from_slice(&[0u8; 20]); // unused
    buf.extend_from_slice(&file_length_words.to_be_bytes()); // file length
    buf.extend_from_slice(&1000_i32.to_le_bytes()); // version
    buf.extend_from_slice(&shape_type.to_le_bytes()); // shape type
    buf.extend_from_slice(&[0u8; 64]); // bbox + z/m (zeros)

    for &(offset_bytes, content_len_bytes) in record_offsets {
        let offset_words = (offset_bytes / 2) as i32;
        let content_words = (content_len_bytes / 2) as i32;
        buf.extend_from_slice(&offset_words.to_be_bytes());
        buf.extend_from_slice(&content_words.to_be_bytes());
    }

    buf
}

/// Pad a string right with spaces to a fixed length
pub fn pad_right(s: &str, len: usize) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.resize(len, b' ');
    v
}

/// Pad a string left with spaces to a fixed length (for numeric fields)
pub fn pad_left(s: &str, len: usize) -> Vec<u8> {
    let mut v = vec![b' '; len.saturating_sub(s.len())];
    v.extend_from_slice(s.as_bytes());
    v
}

/// Write test shapefile set to a directory and return the .shp path.
/// Creates .shp, .dbf, and optionally .shx and .prj files.
pub fn write_test_shapefile(
    dir: &Path,
    name: &str,
    shp_bytes: &[u8],
    dbf_bytes: &[u8],
    shx_bytes: Option<&[u8]>,
    prj_wkt: Option<&str>,
) -> PathBuf {
    use std::fs;

    let shp_path = dir.join(format!("{name}.shp"));
    let dbf_path = dir.join(format!("{name}.dbf"));
    fs::write(&shp_path, shp_bytes).unwrap();
    fs::write(&dbf_path, dbf_bytes).unwrap();

    if let Some(shx) = shx_bytes {
        let shx_path = dir.join(format!("{name}.shx"));
        fs::write(&shx_path, shx).unwrap();
    }

    if let Some(wkt) = prj_wkt {
        let prj_path = dir.join(format!("{name}.prj"));
        fs::write(&prj_path, wkt).unwrap();
    }

    shp_path
}
