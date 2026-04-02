/// Coordinate Reference System parsed from a .prj file (OGC WKT format).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crs {
    /// The raw WKT string describing the coordinate reference system.
    pub wkt: String,
}

impl Crs {
    /// Extract CRS name from WKT (simple implementation)
    pub fn name(&self) -> Option<&str> {
        let start = self.wkt.find('"')? + 1;
        let end = self.wkt[start..].find('"')? + start;
        Some(&self.wkt[start..end])
    }

    /// Whether this is a geographic CRS (degree units).
    /// Returns true if WKT contains GEOGCS and does not contain PROJCS.
    pub fn is_geographic(&self) -> bool {
        self.wkt.contains("GEOGCS") && !self.wkt.contains("PROJCS")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crs_is_geographic_geogcs() {
        let crs = Crs {
            wkt: r#"GEOGCS["GCS_JGD_2011",DATUM["D_JGD_2011"]]"#.to_string(),
        };
        assert!(crs.is_geographic());
    }

    #[test]
    fn test_crs_is_geographic_projcs() {
        let crs = Crs {
            wkt: r#"PROJCS["JGD2011",GEOGCS["GCS_JGD_2011"]]"#.to_string(),
        };
        assert!(!crs.is_geographic());
    }

    #[test]
    fn test_crs_name_extraction() {
        let crs = Crs {
            wkt: r#"GEOGCS["GCS_JGD_2011",DATUM["D_JGD_2011"]]"#.to_string(),
        };
        assert_eq!(crs.name(), Some("GCS_JGD_2011"));
    }
}
