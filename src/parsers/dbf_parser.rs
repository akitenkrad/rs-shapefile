use std::io::{Read, Seek};

use crate::error::ShapefileError;
use crate::io::binary_reader::BinaryReader;
use crate::models::attribute::{AttributeValue, FieldDef, FieldType};

pub(crate) struct DbfHeader {
    pub(crate) num_records: u32,
    pub(crate) record_size: u16,
    pub(crate) fields: Vec<FieldDef>,
}

pub(crate) struct DbfParser<R: Read + Seek> {
    reader: BinaryReader<R>,
    encoding: &'static encoding_rs::Encoding,
    header: Option<DbfHeader>,
    records_read: u32,
}

pub(crate) struct DbfRecordIter<'a, R: Read + Seek> {
    parser: &'a mut DbfParser<R>,
}

impl<'a, R: Read + Seek> Iterator for DbfRecordIter<'a, R> {
    type Item = Result<Vec<(String, AttributeValue)>, ShapefileError>;

    fn next(&mut self) -> Option<Self::Item> {
        let header = self.parser.header.as_ref()?;
        let num_records = header.num_records;
        let record_size = header.record_size as usize;

        loop {
            if self.parser.records_read >= num_records {
                return None;
            }

            // Read deletion flag
            let flag_bytes = match self.parser.reader.read_bytes(1) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let flag = flag_bytes[0];

            if flag == 0x2A {
                // Deleted record: skip remaining bytes
                let skip = record_size - 1;
                if skip > 0 {
                    if let Err(e) = self.parser.reader.read_bytes(skip) {
                        return Some(Err(e));
                    }
                }
                self.parser.records_read += 1;
                continue;
            }

            // Valid record (0x20 or treat any non-deleted as valid)
            let data = match self.parser.reader.read_bytes(record_size - 1) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            self.parser.records_read += 1;

            // Parse fields
            let header = self.parser.header.as_ref().unwrap();
            let mut record = Vec::with_capacity(header.fields.len());
            let mut offset = 0usize;
            for field in &header.fields {
                let len = field.length as usize;
                let raw = &data[offset..offset + len];
                offset += len;
                match self.parser.parse_field_value(field, raw) {
                    Ok(pair) => record.push(pair),
                    Err(e) => return Some(Err(e)),
                }
            }

            return Some(Ok(record));
        }
    }
}

impl<R: Read + Seek> DbfParser<R> {
    pub fn new(
        dbf: R,
        encoding: Option<&'static encoding_rs::Encoding>,
    ) -> Result<Self, ShapefileError> {
        Ok(DbfParser {
            reader: BinaryReader::new(dbf),
            encoding: encoding.unwrap_or(encoding_rs::SHIFT_JIS),
            header: None,
            records_read: 0,
        })
    }

    pub fn parse_header(&mut self) -> Result<DbfHeader, ShapefileError> {
        // 1. Version byte (1 byte, skip)
        let _version = self.reader.read_bytes(1)?;

        // 2. Date (3 bytes, skip)
        let _date = self.reader.read_bytes(3)?;

        // 3. Number of records (i32 LE)
        let num_records = self.reader.read_i32_le()? as u32;

        // 4. Header size (i16 LE)
        let header_size = self.reader.read_i16_le()? as u16;

        // 5. Record size (i16 LE)
        let record_size = self.reader.read_i16_le()? as u16;

        // 6. Skip 20 reserved bytes
        let _reserved = self.reader.read_bytes(20)?;

        // 7. Calculate number of fields: (header_size - 33) / 32
        let num_fields = (header_size as usize).saturating_sub(33) / 32;

        // 8. Read field descriptors
        let mut fields = Vec::with_capacity(num_fields);
        for _ in 0..num_fields {
            let field_bytes = self.reader.read_bytes(32)?;

            // name: first 11 bytes, trim null bytes
            let name_bytes = &field_bytes[0..11];
            let name = name_bytes
                .iter()
                .take_while(|&&b| b != 0)
                .copied()
                .collect::<Vec<u8>>();
            let name = String::from_utf8(name).map_err(|e| ShapefileError::CorruptedFile {
                reason: format!("invalid field name encoding: {e}"),
            })?;

            // field_type: byte at offset 11
            let type_byte = field_bytes[11];
            let field_type = match type_byte {
                b'C' => FieldType::Character,
                b'N' => FieldType::Numeric,
                b'F' => FieldType::Float,
                b'D' => FieldType::Date,
                b'L' => FieldType::Logical,
                _ => {
                    return Err(ShapefileError::CorruptedFile {
                        reason: format!("unknown field type: 0x{type_byte:02X}"),
                    })
                }
            };

            // length: byte at offset 16
            let length = field_bytes[16];

            // decimal_count: byte at offset 17
            let decimal_count = field_bytes[17];

            fields.push(FieldDef {
                name,
                field_type,
                length,
                decimal_count,
            });
        }

        // 9. Read and verify terminator byte (0x0D)
        let terminator = self.reader.read_bytes(1)?;
        if terminator[0] != 0x0D {
            return Err(ShapefileError::CorruptedFile {
                reason: format!(
                    "expected field descriptor terminator 0x0D, got 0x{:02X}",
                    terminator[0]
                ),
            });
        }

        let header = DbfHeader {
            num_records,
            record_size,
            fields,
        };

        // Store header for iteration
        self.header = Some(DbfHeader {
            num_records,
            record_size,
            fields: header.fields.clone(),
        });
        self.records_read = 0;

        Ok(header)
    }

    fn parse_field_value(
        &self,
        field: &FieldDef,
        raw_bytes: &[u8],
    ) -> Result<(String, AttributeValue), ShapefileError> {
        let name = field.name.clone();
        let value = match field.field_type {
            FieldType::Character => {
                let (decoded, _enc, had_errors) = self.encoding.decode(raw_bytes);
                if had_errors {
                    return Err(ShapefileError::EncodingError {
                        field: name,
                        reason: "failed to decode character field bytes".to_string(),
                    });
                }
                let s = decoded.trim_end().to_string();
                if s.is_empty() {
                    AttributeValue::Null
                } else {
                    AttributeValue::Text(s)
                }
            }
            FieldType::Numeric | FieldType::Float => {
                // Numeric fields are ASCII
                let s = String::from_utf8_lossy(raw_bytes);
                let trimmed = s.trim();
                if trimmed.is_empty() || trimmed.chars().all(|c| c == '*') {
                    AttributeValue::Null
                } else {
                    let v: f64 = trimmed.parse().map_err(|_| ShapefileError::CorruptedFile {
                        reason: format!(
                            "failed to parse numeric field '{}': '{}'",
                            field.name, trimmed
                        ),
                    })?;
                    AttributeValue::Numeric(v)
                }
            }
            FieldType::Date => {
                let s = String::from_utf8_lossy(raw_bytes);
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() {
                    AttributeValue::Null
                } else {
                    AttributeValue::Date(trimmed)
                }
            }
            FieldType::Logical => {
                let byte = raw_bytes[0];
                match byte {
                    b'T' | b't' | b'Y' | b'y' => AttributeValue::Logical(true),
                    b'F' | b'f' | b'N' | b'n' => AttributeValue::Logical(false),
                    _ => AttributeValue::Null,
                }
            }
        };
        Ok((name, value))
    }

    pub fn iter_records(&mut self) -> DbfRecordIter<'_, R> {
        DbfRecordIter { parser: self }
    }

    /// Seek to the start of record data (after header).
    /// Call this before iter_records if you need to re-iterate.
    pub fn seek_to_records(&mut self) -> Result<(), ShapefileError> {
        if let Some(header) = &self.header {
            // header_size includes everything up to the first record
            // We need to calculate it from fields count
            let header_size = 32 + (header.fields.len() * 32) + 1;
            self.reader.seek_from_start(header_size as u64)?;
            self.records_read = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal DBF file for testing.
    /// `fields`: slice of (name, type_byte, length, decimal_count)
    /// `records`: slice of (deletion_flag_byte, field_values)
    ///   where each field_value is a Vec<u8> of exactly `length` bytes
    fn build_test_dbf(fields: &[(&str, u8, u8, u8)], records: &[(u8, &[Vec<u8>])]) -> Vec<u8> {
        let mut buf = Vec::new();

        // Calculate sizes
        let num_fields = fields.len();
        let header_size: u16 = 32 + (num_fields as u16 * 32) + 1; // main header + field descs + terminator
        let record_size: u16 = 1 + fields.iter().map(|f| f.2 as u16).sum::<u16>(); // deletion flag + sum of field lengths

        // Main header (32 bytes)
        buf.push(0x03); // version: dBASE III
        buf.extend_from_slice(&[24, 1, 1]); // date: 2024-01-01
        buf.extend_from_slice(&(records.len() as i32).to_le_bytes()); // num_records
        buf.extend_from_slice(&(header_size as i16).to_le_bytes()); // header_size
        buf.extend_from_slice(&(record_size as i16).to_le_bytes()); // record_size
        buf.extend_from_slice(&[0u8; 20]); // reserved

        // Field descriptors (32 bytes each)
        for (name, type_byte, length, decimal_count) in fields {
            let mut name_bytes = [0u8; 11];
            for (i, b) in name.as_bytes().iter().enumerate().take(11) {
                name_bytes[i] = *b;
            }
            buf.extend_from_slice(&name_bytes); // name (11 bytes)
            buf.push(*type_byte); // field type (1 byte)
            buf.extend_from_slice(&[0u8; 4]); // reserved (4 bytes)
            buf.push(*length); // field length (1 byte)
            buf.push(*decimal_count); // decimal count (1 byte)
            buf.extend_from_slice(&[0u8; 14]); // reserved (14 bytes)
        }

        // Terminator
        buf.push(0x0D);

        // Records
        for (flag, field_values) in records {
            buf.push(*flag);
            for val in *field_values {
                buf.extend_from_slice(val);
            }
        }

        buf
    }

    /// Helper to pad a string to a fixed length with trailing spaces
    fn pad_right(s: &str, len: usize) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.resize(len, b' ');
        v
    }

    /// Helper to pad a string to a fixed length with leading spaces (for numeric)
    fn pad_left(s: &str, len: usize) -> Vec<u8> {
        let mut v = vec![b' '; len.saturating_sub(s.len())];
        v.extend_from_slice(s.as_bytes());
        v
    }

    #[test]
    fn test_parse_field_name() {
        let dbf_bytes = build_test_dbf(
            &[("TEST_FIELD", b'C', 10, 0)],
            &[(0x20, &[pad_right("Hello", 10)])],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        let header = parser.parse_header().unwrap();
        assert_eq!(header.fields.len(), 1);
        assert_eq!(header.fields[0].name, "TEST_FIELD");
        assert_eq!(header.fields[0].field_type, FieldType::Character);
        assert_eq!(header.fields[0].length, 10);
    }

    #[test]
    fn test_parse_character_field() {
        let dbf_bytes = build_test_dbf(
            &[("NAME", b'C', 10, 0)],
            &[(0x20, &[pad_right("Hello", 10)])],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].0, "NAME");
        assert_eq!(rec[0].1, AttributeValue::Text("Hello".to_string()));
    }

    #[test]
    fn test_parse_numeric_field() {
        let dbf_bytes = build_test_dbf(
            &[("VALUE", b'N', 10, 1)],
            &[(0x20, &[pad_left("42.5", 10)])],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].0, "VALUE");
        assert_eq!(rec[0].1, AttributeValue::Numeric(42.5));
    }

    #[test]
    fn test_parse_date_field() {
        let dbf_bytes = build_test_dbf(
            &[("CREATED", b'D', 8, 0)],
            &[(0x20, &[b"20240101".to_vec()])],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].0, "CREATED");
        assert_eq!(rec[0].1, AttributeValue::Date("20240101".to_string()));
    }

    #[test]
    fn test_deleted_record_skipped() {
        let dbf_bytes = build_test_dbf(
            &[("NAME", b'C', 5, 0)],
            &[
                (0x2A, &[pad_right("DEL", 5)]),  // deleted
                (0x20, &[pad_right("KEEP", 5)]), // valid
            ],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Text("KEEP".to_string()));
    }

    #[test]
    fn test_encoding_utf8_from_cpg() {
        let dbf_bytes = build_test_dbf(
            &[("CITY", b'C', 10, 0)],
            &[(0x20, &[pad_right("Tokyo", 10)])],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Text("Tokyo".to_string()));
    }

    #[test]
    fn test_encoding_shiftjis_fallback() {
        // "東京" in Shift-JIS: 0x93 0x8C 0x8B 0x9E
        let mut sjis_bytes = vec![0x93, 0x8C, 0x8B, 0x9E];
        // Pad to field length of 10
        sjis_bytes.resize(10, b' ');

        let dbf_bytes = build_test_dbf(&[("CITY", b'C', 10, 0)], &[(0x20, &[sjis_bytes])]);
        // Use default encoding (SHIFT_JIS) by passing None
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), None).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Text("東京".to_string()));
    }

    #[test]
    fn test_parse_logical_field_true() {
        let dbf_bytes = build_test_dbf(&[("FLAG", b'L', 1, 0)], &[(0x20, &[b"T".to_vec()])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Logical(true));
    }

    #[test]
    fn test_parse_logical_field_false() {
        let dbf_bytes = build_test_dbf(&[("FLAG", b'L', 1, 0)], &[(0x20, &[b"F".to_vec()])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Logical(false));
    }

    #[test]
    fn test_parse_logical_field_yes() {
        let dbf_bytes = build_test_dbf(&[("FLAG", b'L', 1, 0)], &[(0x20, &[b"Y".to_vec()])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Logical(true));
    }

    #[test]
    fn test_parse_logical_field_no() {
        let dbf_bytes = build_test_dbf(&[("FLAG", b'L', 1, 0)], &[(0x20, &[b"N".to_vec()])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Logical(false));
    }

    #[test]
    fn test_parse_logical_field_null() {
        // '?' or space should result in Null
        let dbf_bytes = build_test_dbf(&[("FLAG", b'L', 1, 0)], &[(0x20, &[b"?".to_vec()])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Null);
    }

    #[test]
    fn test_parse_float_field() {
        let dbf_bytes = build_test_dbf(
            &[("RATIO", b'F', 10, 4)],
            &[(0x20, &[pad_left("3.1415", 10)])],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].0, "RATIO");
        match &rec[0].1 {
            AttributeValue::Numeric(v) => assert!((*v - 3.1415).abs() < 1e-10),
            other => panic!("expected Numeric, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_numeric_null_empty() {
        let dbf_bytes = build_test_dbf(&[("VAL", b'N', 10, 0)], &[(0x20, &[pad_right("", 10)])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Null);
    }

    #[test]
    fn test_parse_numeric_null_asterisks() {
        let dbf_bytes = build_test_dbf(&[("VAL", b'N', 5, 0)], &[(0x20, &[b"*****".to_vec()])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Null);
    }

    #[test]
    fn test_parse_character_null_empty() {
        let dbf_bytes = build_test_dbf(&[("NAME", b'C', 5, 0)], &[(0x20, &[pad_right("", 5)])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Null);
    }

    #[test]
    fn test_parse_date_null_empty() {
        let dbf_bytes = build_test_dbf(&[("DT", b'D', 8, 0)], &[(0x20, &[pad_right("", 8)])]);
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec[0].1, AttributeValue::Null);
    }

    #[test]
    fn test_multiple_fields_in_one_record() {
        let dbf_bytes = build_test_dbf(
            &[
                ("NAME", b'C', 10, 0),
                ("VALUE", b'N', 10, 2),
                ("ACTIVE", b'L', 1, 0),
            ],
            &[(
                0x20,
                &[
                    pad_right("Tokyo", 10),
                    pad_left("100.50", 10),
                    b"T".to_vec(),
                ],
            )],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 1);
        let rec = records[0].as_ref().unwrap();
        assert_eq!(rec.len(), 3);
        assert_eq!(rec[0].1, AttributeValue::Text("Tokyo".to_string()));
        assert_eq!(rec[1].1, AttributeValue::Numeric(100.50));
        assert_eq!(rec[2].1, AttributeValue::Logical(true));
    }

    #[test]
    fn test_seek_to_records_and_reiterate() {
        let dbf_bytes = build_test_dbf(
            &[("NAME", b'C', 5, 0)],
            &[
                (0x20, &[pad_right("One", 5)]),
                (0x20, &[pad_right("Two", 5)]),
            ],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();

        // First iteration
        let records1: Vec<_> = parser.iter_records().collect();
        assert_eq!(records1.len(), 2);

        // Seek back and iterate again
        parser.seek_to_records().unwrap();
        let records2: Vec<_> = parser.iter_records().collect();
        assert_eq!(records2.len(), 2);
    }

    #[test]
    fn test_multiple_records() {
        let dbf_bytes = build_test_dbf(
            &[("VAL", b'N', 5, 0)],
            &[
                (0x20, &[pad_left("1", 5)]),
                (0x20, &[pad_left("2", 5)]),
                (0x20, &[pad_left("3", 5)]),
            ],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        parser.parse_header().unwrap();
        let records: Vec<_> = parser.iter_records().collect();
        assert_eq!(records.len(), 3);
        let rec0 = records[0].as_ref().unwrap();
        let rec2 = records[2].as_ref().unwrap();
        assert_eq!(rec0[0].1, AttributeValue::Numeric(1.0));
        assert_eq!(rec2[0].1, AttributeValue::Numeric(3.0));
    }

    #[test]
    fn test_parse_logical_lowercase() {
        // Test lowercase variants: t, f, y, n
        for (input, expected) in [
            (b"t", AttributeValue::Logical(true)),
            (b"f", AttributeValue::Logical(false)),
            (b"y", AttributeValue::Logical(true)),
            (b"n", AttributeValue::Logical(false)),
        ] {
            let dbf_bytes = build_test_dbf(&[("FLAG", b'L', 1, 0)], &[(0x20, &[input.to_vec()])]);
            let mut parser =
                DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
            parser.parse_header().unwrap();
            let records: Vec<_> = parser.iter_records().collect();
            let rec = records[0].as_ref().unwrap();
            assert_eq!(rec[0].1, expected, "failed for input {:?}", input);
        }
    }

    #[test]
    fn test_header_num_records() {
        let dbf_bytes = build_test_dbf(
            &[("X", b'C', 3, 0)],
            &[
                (0x20, &[pad_right("a", 3)]),
                (0x20, &[pad_right("b", 3)]),
                (0x20, &[pad_right("c", 3)]),
            ],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        let header = parser.parse_header().unwrap();
        assert_eq!(header.num_records, 3);
    }

    #[test]
    fn test_header_record_size() {
        // Record size = 1 (deletion flag) + field lengths
        let dbf_bytes = build_test_dbf(
            &[("A", b'C', 10, 0), ("B", b'N', 5, 0)],
            &[(0x20, &[pad_right("x", 10), pad_left("1", 5)])],
        );
        let mut parser = DbfParser::new(Cursor::new(dbf_bytes), Some(encoding_rs::UTF_8)).unwrap();
        let header = parser.parse_header().unwrap();
        assert_eq!(header.record_size, 16); // 1 + 10 + 5
    }
}
