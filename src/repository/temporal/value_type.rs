//! The column an attribute's recorded values are stored in.

use strum::{Display, EnumString};

/// Typed value discriminator for temporal attribute values.
///
/// Names the `attribute_history` column a value lands in, which is a storage
/// decision rather than an NGSI-LD one: the specification models an attribute
/// value as JSON, and splitting it across typed columns is what makes the
/// history table's partial indexes usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "PascalCase")]
pub enum ValueType {
    /// A JSON number.
    Number,
    /// A JSON string with no further NGSI-LD meaning.
    String,
    /// A JSON boolean.
    Boolean,
    /// A GeoJSON geometry (clause 4.7).
    Geometry,
    /// A JSON object or array, stored verbatim.
    Object,
    /// An RFC 3339 timestamp.
    DateTime,
    /// A URI.
    Uri,
}

impl ValueType {
    /// Returns the canonical string representation used in persistence.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Number => "Number",
            Self::String => "String",
            Self::Boolean => "Boolean",
            Self::Geometry => "Geometry",
            Self::Object => "Object",
            Self::DateTime => "DateTime",
            Self::Uri => "Uri",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::repository::temporal::value_type::ValueType;
    use std::str::FromStr;

    #[test]
    fn value_type_display_and_parse_round_trip() {
        for variant in [
            ValueType::Number,
            ValueType::String,
            ValueType::Boolean,
            ValueType::Geometry,
            ValueType::Object,
            ValueType::DateTime,
            ValueType::Uri,
        ] {
            let rendered = variant.to_string();
            let parsed = ValueType::from_str(&rendered).expect("parse valueType");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
            pretty_assertions::assert_eq!(variant.as_str(), rendered);
        }
    }
}
