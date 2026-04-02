use std::io::{Read, Seek, SeekFrom};

use crate::error::ShapefileError;

pub(crate) struct BinaryReader<R: Read + Seek> {
    inner: R,
}

impl<R: Read + Seek> BinaryReader<R> {
    pub fn new(inner: R) -> Self {
        BinaryReader { inner }
    }

    pub fn read_i32_be(&mut self) -> Result<i32, ShapefileError> {
        let mut buf = [0u8; 4];
        self.inner.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ShapefileError::CorruptedFile {
                    reason: "unexpected EOF while reading i32 (big-endian)".to_string(),
                }
            } else {
                ShapefileError::Io(e)
            }
        })?;
        Ok(i32::from_be_bytes(buf))
    }

    pub fn read_i32_le(&mut self) -> Result<i32, ShapefileError> {
        let mut buf = [0u8; 4];
        self.inner.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ShapefileError::CorruptedFile {
                    reason: "unexpected EOF while reading i32 (little-endian)".to_string(),
                }
            } else {
                ShapefileError::Io(e)
            }
        })?;
        Ok(i32::from_le_bytes(buf))
    }

    pub fn read_i16_le(&mut self) -> Result<i16, ShapefileError> {
        let mut buf = [0u8; 2];
        self.inner.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ShapefileError::CorruptedFile {
                    reason: "unexpected EOF while reading i16 (little-endian)".to_string(),
                }
            } else {
                ShapefileError::Io(e)
            }
        })?;
        Ok(i16::from_le_bytes(buf))
    }

    pub fn read_f64_le(&mut self) -> Result<f64, ShapefileError> {
        let mut buf = [0u8; 8];
        self.inner.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ShapefileError::CorruptedFile {
                    reason: "unexpected EOF while reading f64 (little-endian)".to_string(),
                }
            } else {
                ShapefileError::Io(e)
            }
        })?;
        Ok(f64::from_le_bytes(buf))
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, ShapefileError> {
        let mut buf = vec![0u8; n];
        self.inner.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ShapefileError::CorruptedFile {
                    reason: format!("unexpected EOF while reading {n} bytes"),
                }
            } else {
                ShapefileError::Io(e)
            }
        })?;
        Ok(buf)
    }

    pub fn read_string(&mut self, n: usize) -> Result<String, ShapefileError> {
        let bytes = self.read_bytes(n)?;
        String::from_utf8(bytes).map_err(|e| ShapefileError::CorruptedFile {
            reason: format!("invalid UTF-8: {e}"),
        })
    }

    pub fn seek_from_start(&mut self, offset: u64) -> Result<(), ShapefileError> {
        self.inner.seek(SeekFrom::Start(offset))?;
        Ok(())
    }

    pub fn position(&mut self) -> Result<u64, ShapefileError> {
        Ok(self.inner.stream_position()?)
    }

    pub fn is_eof(&mut self) -> Result<bool, ShapefileError> {
        let current = self.inner.stream_position()?;
        let end = self.inner.seek(SeekFrom::End(0))?;
        self.inner.seek(SeekFrom::Start(current))?;
        Ok(current >= end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_i32_be() {
        let data = 9994_i32.to_be_bytes();
        let mut reader = BinaryReader::new(Cursor::new(data.to_vec()));
        assert_eq!(reader.read_i32_be().unwrap(), 9994);
    }

    #[test]
    fn test_read_i32_le() {
        let data = 1000_i32.to_le_bytes();
        let mut reader = BinaryReader::new(Cursor::new(data.to_vec()));
        assert_eq!(reader.read_i32_le().unwrap(), 1000);
    }

    #[test]
    fn test_read_f64_le() {
        let data = 139.6917_f64.to_le_bytes();
        let mut reader = BinaryReader::new(Cursor::new(data.to_vec()));
        let val = reader.read_f64_le().unwrap();
        assert!((val - 139.6917).abs() < 1e-10);
    }

    #[test]
    fn test_eof_returns_error() {
        let data: Vec<u8> = vec![0, 1]; // only 2 bytes
        let mut reader = BinaryReader::new(Cursor::new(data));
        let result = reader.read_i32_be();
        assert!(matches!(result, Err(ShapefileError::CorruptedFile { .. })));
    }

    #[test]
    fn test_seek_and_position() {
        let data = vec![0u8; 100];
        let mut reader = BinaryReader::new(Cursor::new(data));
        reader.seek_from_start(50).unwrap();
        assert_eq!(reader.position().unwrap(), 50);
    }

    #[test]
    fn test_is_eof_false() {
        let data = vec![0u8; 10];
        let mut reader = BinaryReader::new(Cursor::new(data));
        assert!(!reader.is_eof().unwrap());
    }

    #[test]
    fn test_is_eof_true() {
        let data = vec![0u8; 4];
        let mut reader = BinaryReader::new(Cursor::new(data));
        reader.read_i32_le().unwrap(); // consume all bytes
        assert!(reader.is_eof().unwrap());
    }

    #[test]
    fn test_read_i16_le() {
        let data = 12345_i16.to_le_bytes();
        let mut reader = BinaryReader::new(Cursor::new(data.to_vec()));
        assert_eq!(reader.read_i16_le().unwrap(), 12345);
    }

    #[test]
    fn test_read_i16_le_negative() {
        let data = (-100_i16).to_le_bytes();
        let mut reader = BinaryReader::new(Cursor::new(data.to_vec()));
        assert_eq!(reader.read_i16_le().unwrap(), -100);
    }

    #[test]
    fn test_read_bytes() {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let mut reader = BinaryReader::new(Cursor::new(data.clone()));
        assert_eq!(reader.read_bytes(4).unwrap(), data);
    }

    #[test]
    fn test_read_bytes_partial() {
        let data = vec![1, 2, 3, 4, 5];
        let mut reader = BinaryReader::new(Cursor::new(data));
        assert_eq!(reader.read_bytes(3).unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_read_string_valid_utf8() {
        let data = b"hello".to_vec();
        let mut reader = BinaryReader::new(Cursor::new(data));
        assert_eq!(reader.read_string(5).unwrap(), "hello");
    }

    #[test]
    fn test_read_string_invalid_utf8() {
        let data = vec![0xFF, 0xFE, 0xFD];
        let mut reader = BinaryReader::new(Cursor::new(data));
        let result = reader.read_string(3);
        assert!(matches!(result, Err(ShapefileError::CorruptedFile { .. })));
    }

    #[test]
    fn test_eof_on_f64() {
        // Only 4 bytes available, need 8 for f64
        let data = vec![0u8; 4];
        let mut reader = BinaryReader::new(Cursor::new(data));
        let result = reader.read_f64_le();
        assert!(matches!(result, Err(ShapefileError::CorruptedFile { .. })));
    }

    #[test]
    fn test_eof_on_i16() {
        // Only 1 byte available, need 2 for i16
        let data = vec![0u8; 1];
        let mut reader = BinaryReader::new(Cursor::new(data));
        let result = reader.read_i16_le();
        assert!(matches!(result, Err(ShapefileError::CorruptedFile { .. })));
    }

    #[test]
    fn test_eof_on_read_bytes() {
        // Only 2 bytes available, request 5
        let data = vec![0u8; 2];
        let mut reader = BinaryReader::new(Cursor::new(data));
        let result = reader.read_bytes(5);
        assert!(matches!(result, Err(ShapefileError::CorruptedFile { .. })));
    }

    #[test]
    fn test_eof_on_i32_le() {
        let data = vec![0u8; 2];
        let mut reader = BinaryReader::new(Cursor::new(data));
        let result = reader.read_i32_le();
        assert!(matches!(result, Err(ShapefileError::CorruptedFile { .. })));
    }

    #[test]
    fn test_seek_from_start_and_read() {
        let mut data = vec![0u8; 8];
        // Put a known i32 at offset 4
        let val_bytes = 42_i32.to_le_bytes();
        data[4..8].copy_from_slice(&val_bytes);
        let mut reader = BinaryReader::new(Cursor::new(data));
        reader.seek_from_start(4).unwrap();
        assert_eq!(reader.read_i32_le().unwrap(), 42);
    }

    #[test]
    fn test_position_after_reads() {
        let data = vec![0u8; 16];
        let mut reader = BinaryReader::new(Cursor::new(data));
        assert_eq!(reader.position().unwrap(), 0);
        reader.read_i32_le().unwrap();
        assert_eq!(reader.position().unwrap(), 4);
        reader.read_f64_le().unwrap();
        assert_eq!(reader.position().unwrap(), 12);
    }
}
