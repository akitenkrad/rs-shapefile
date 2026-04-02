use std::collections::HashMap;

use crate::models::attribute::AttributeValue;
use crate::models::geometry::Geometry;

/// A single record from a shapefile, combining geometry with DBF attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRecord {
    /// 1-based record number as stored in the .shp file.
    pub record_number: u32,
    /// The geometry (point, polyline, polygon, etc.) of this record.
    pub geometry: Geometry,
    /// Attribute key-value pairs from the .dbf file.
    pub attributes: HashMap<String, AttributeValue>,
}

impl ShapeRecord {
    /// Returns a reference to the attribute value for the given field name, if present.
    pub fn get_attr(&self, field: &str) -> Option<&AttributeValue> {
        self.attributes.get(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_attr_found() {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), AttributeValue::Text("test".to_string()));
        let rec = ShapeRecord {
            record_number: 1,
            geometry: Geometry::Null,
            attributes: attrs,
        };
        assert_eq!(
            rec.get_attr("name"),
            Some(&AttributeValue::Text("test".to_string()))
        );
    }

    #[test]
    fn test_get_attr_not_found() {
        let rec = ShapeRecord {
            record_number: 1,
            geometry: Geometry::Null,
            attributes: HashMap::new(),
        };
        assert_eq!(rec.get_attr("missing"), None);
    }

    #[test]
    fn test_get_attr_numeric() {
        let mut attrs = HashMap::new();
        attrs.insert("population".to_string(), AttributeValue::Numeric(1000.0));
        let rec = ShapeRecord {
            record_number: 2,
            geometry: Geometry::Null,
            attributes: attrs,
        };
        assert_eq!(
            rec.get_attr("population"),
            Some(&AttributeValue::Numeric(1000.0))
        );
    }

    #[test]
    fn test_get_attr_null_value() {
        let mut attrs = HashMap::new();
        attrs.insert("empty".to_string(), AttributeValue::Null);
        let rec = ShapeRecord {
            record_number: 3,
            geometry: Geometry::Null,
            attributes: attrs,
        };
        assert_eq!(rec.get_attr("empty"), Some(&AttributeValue::Null));
    }
}
