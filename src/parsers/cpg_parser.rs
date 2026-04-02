use std::io::Read;

use crate::error::ShapefileError;

pub(crate) struct CpgParser<R: Read> {
    reader: R,
}

impl<R: Read> CpgParser<R> {
    pub fn new(cpg: R) -> Self {
        CpgParser { reader: cpg }
    }

    pub fn parse(&mut self) -> Result<&'static encoding_rs::Encoding, ShapefileError> {
        let mut content = String::new();
        self.reader.read_to_string(&mut content)?;
        let label = content.trim();
        Ok(encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::SHIFT_JIS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_utf8_cpg() {
        let mut parser = CpgParser::new(Cursor::new(b"UTF-8".to_vec()));
        let enc = parser.parse().unwrap();
        assert_eq!(enc, encoding_rs::UTF_8);
    }

    #[test]
    fn test_parse_shiftjis_cpg() {
        let mut parser = CpgParser::new(Cursor::new(b"Shift_JIS".to_vec()));
        let enc = parser.parse().unwrap();
        assert_eq!(enc, encoding_rs::SHIFT_JIS);
    }
}
