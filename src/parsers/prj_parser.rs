use std::io::Read;

use crate::error::ShapefileError;
use crate::models::crs::Crs;

pub(crate) struct PrjParser<R: Read> {
    reader: R,
}

impl<R: Read> PrjParser<R> {
    pub fn new(prj: R) -> Self {
        PrjParser { reader: prj }
    }

    pub fn parse(&mut self) -> Result<Crs, ShapefileError> {
        let mut wkt = String::new();
        self.reader.read_to_string(&mut wkt)?;
        Ok(Crs { wkt })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_prj_wkt() {
        let wkt = r#"GEOGCS["GCS_JGD_2011",DATUM["D_JGD_2011"]]"#;
        let mut parser = PrjParser::new(Cursor::new(wkt.as_bytes().to_vec()));
        let crs = parser.parse().unwrap();
        assert_eq!(crs.name(), Some("GCS_JGD_2011"));
    }
}
