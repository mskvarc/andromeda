//! The rendering an NGSI-LD response uses for an entity (clauses 4.5.1 and 4.5.4).

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Response format for entity queries (ETSI GS CIM 009 clauses 4.5.1 and 4.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum ResponseFormat {
    /// Full NGSI-LD normalized representation.
    #[default]
    Normalized,
    /// Key-values representation.
    #[strum(serialize = "keyValues")]
    #[serde(rename = "keyValues")]
    KeyValues,
    /// Simplified representation with flat key-value pairs.
    Simplified,
    /// Concise representation omitting redundant type information.
    Concise,
}

impl ResponseFormat {
    /// Whether this format uses a simplified (flat) structure.
    pub const fn is_simplified(self) -> bool {
        matches!(self, ResponseFormat::KeyValues | ResponseFormat::Simplified)
    }
}

#[cfg(test)]
mod tests {
    use crate::response_format::ResponseFormat;
    use std::str::FromStr;

    #[test]
    fn response_format_default_is_normalized() {
        pretty_assertions::assert_eq!(ResponseFormat::default(), ResponseFormat::Normalized);
    }

    #[test]
    fn response_format_display_and_parse_round_trip() {
        for variant in [
            ResponseFormat::Normalized,
            ResponseFormat::KeyValues,
            ResponseFormat::Simplified,
            ResponseFormat::Concise,
        ] {
            let rendered = variant.to_string();
            let parsed = ResponseFormat::from_str(&rendered).expect("round trip");
            pretty_assertions::assert_eq!(variant, parsed, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn response_format_serde_round_trip() {
        for variant in [
            ResponseFormat::Normalized,
            ResponseFormat::KeyValues,
            ResponseFormat::Simplified,
            ResponseFormat::Concise,
        ] {
            let json = serde_json::to_string(&variant).expect("serialises");
            let parsed: ResponseFormat = serde_json::from_str(&json).expect("deserialises");
            pretty_assertions::assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn is_simplified() {
        assert!(!ResponseFormat::Normalized.is_simplified());
        assert!(ResponseFormat::KeyValues.is_simplified());
        assert!(ResponseFormat::Simplified.is_simplified());
        assert!(!ResponseFormat::Concise.is_simplified());
    }
}
