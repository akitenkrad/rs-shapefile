/// Field type in a dBASE (.dbf) file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// Fixed-length character string.
    Character,
    /// Numeric value stored as ASCII digits.
    Numeric,
    /// Floating-point numeric value.
    Float,
    /// Date stored as YYYYMMDD.
    Date,
    /// Boolean (T/F/Y/N).
    Logical,
}

/// Field definition from DBF header
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub length: u8,
    pub decimal_count: u8,
}

impl FieldDef {
    /// Whether this field is numeric (valid for describe())
    pub fn is_numeric(&self) -> bool {
        matches!(self.field_type, FieldType::Numeric | FieldType::Float)
    }
}

/// A single attribute value from a DBF record.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    /// Character string value.
    Text(String),
    /// Numeric (integer or decimal) value.
    Numeric(f64),
    /// Date stored as a "YYYYMMDD" string.
    Date(String),
    /// Boolean value.
    Logical(bool),
    /// Null / empty value.
    Null,
}

impl AttributeValue {
    /// Returns the inner string reference if this is a `Text` value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttributeValue::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the inner `f64` if this is a `Numeric` value.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            AttributeValue::Numeric(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the inner `bool` if this is a `Logical` value.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            AttributeValue::Logical(b) => Some(*b),
            _ => None,
        }
    }

    /// Prefix match (only effective for Text variant)
    pub fn starts_with(&self, prefix: &str) -> bool {
        matches!(self, AttributeValue::Text(s) if s.starts_with(prefix))
    }

    /// Whether this is a numeric value (valid target for describe())
    pub fn is_numeric(&self) -> bool {
        matches!(self, AttributeValue::Numeric(_))
    }
}

/// Descriptive statistics for a numeric field
#[derive(Debug, Clone)]
pub struct FieldStats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub median: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_def_is_numeric_numeric() {
        let fd = FieldDef {
            name: "val".to_string(),
            field_type: FieldType::Numeric,
            length: 10,
            decimal_count: 2,
        };
        assert!(fd.is_numeric());
    }

    #[test]
    fn test_field_def_is_numeric_float() {
        let fd = FieldDef {
            name: "val".to_string(),
            field_type: FieldType::Float,
            length: 10,
            decimal_count: 2,
        };
        assert!(fd.is_numeric());
    }

    #[test]
    fn test_field_def_is_numeric_character() {
        let fd = FieldDef {
            name: "name".to_string(),
            field_type: FieldType::Character,
            length: 50,
            decimal_count: 0,
        };
        assert!(!fd.is_numeric());
    }

    #[test]
    fn test_field_def_is_numeric_date() {
        let fd = FieldDef {
            name: "dt".to_string(),
            field_type: FieldType::Date,
            length: 8,
            decimal_count: 0,
        };
        assert!(!fd.is_numeric());
    }

    #[test]
    fn test_attribute_value_starts_with_text() {
        let v = AttributeValue::Text("ABC".to_string());
        assert!(v.starts_with("A"));
        assert!(!v.starts_with("B"));
    }

    #[test]
    fn test_attribute_value_starts_with_non_text() {
        let v = AttributeValue::Numeric(42.0);
        assert!(!v.starts_with("4"));
    }

    // --- as_str tests ---

    #[test]
    fn test_as_str_on_text() {
        let v = AttributeValue::Text("hello".to_string());
        assert_eq!(v.as_str(), Some("hello"));
    }

    #[test]
    fn test_as_str_on_numeric() {
        assert_eq!(AttributeValue::Numeric(1.0).as_str(), None);
    }

    #[test]
    fn test_as_str_on_null() {
        assert_eq!(AttributeValue::Null.as_str(), None);
    }

    #[test]
    fn test_as_str_on_logical() {
        assert_eq!(AttributeValue::Logical(true).as_str(), None);
    }

    #[test]
    fn test_as_str_on_date() {
        assert_eq!(AttributeValue::Date("20260101".to_string()).as_str(), None);
    }

    // --- as_f64 tests ---

    #[test]
    fn test_as_f64_on_numeric() {
        assert_eq!(AttributeValue::Numeric(42.5).as_f64(), Some(42.5));
    }

    #[test]
    fn test_as_f64_on_text() {
        assert_eq!(AttributeValue::Text("x".to_string()).as_f64(), None);
    }

    #[test]
    fn test_as_f64_on_null() {
        assert_eq!(AttributeValue::Null.as_f64(), None);
    }

    #[test]
    fn test_as_f64_on_logical() {
        assert_eq!(AttributeValue::Logical(false).as_f64(), None);
    }

    #[test]
    fn test_as_f64_on_date() {
        assert_eq!(AttributeValue::Date("20260101".to_string()).as_f64(), None);
    }

    // --- as_bool tests ---

    #[test]
    fn test_as_bool_on_logical_true() {
        assert_eq!(AttributeValue::Logical(true).as_bool(), Some(true));
    }

    #[test]
    fn test_as_bool_on_logical_false() {
        assert_eq!(AttributeValue::Logical(false).as_bool(), Some(false));
    }

    #[test]
    fn test_as_bool_on_null() {
        assert_eq!(AttributeValue::Null.as_bool(), None);
    }

    #[test]
    fn test_as_bool_on_text() {
        assert_eq!(AttributeValue::Text("true".to_string()).as_bool(), None);
    }

    #[test]
    fn test_as_bool_on_numeric() {
        assert_eq!(AttributeValue::Numeric(1.0).as_bool(), None);
    }

    // --- is_numeric tests ---

    #[test]
    fn test_is_numeric_on_numeric() {
        assert!(AttributeValue::Numeric(1.0).is_numeric());
    }

    #[test]
    fn test_is_numeric_on_text() {
        assert!(!AttributeValue::Text("x".to_string()).is_numeric());
    }

    #[test]
    fn test_is_numeric_on_null() {
        assert!(!AttributeValue::Null.is_numeric());
    }

    #[test]
    fn test_is_numeric_on_logical() {
        assert!(!AttributeValue::Logical(true).is_numeric());
    }

    #[test]
    fn test_is_numeric_on_date() {
        assert!(!AttributeValue::Date("20260101".to_string()).is_numeric());
    }

    // --- Date variant coverage ---

    #[test]
    fn test_date_starts_with() {
        let v = AttributeValue::Date("20260401".to_string());
        // Date is not Text, so starts_with should return false
        assert!(!v.starts_with("2026"));
    }
}
