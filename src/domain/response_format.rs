//! NGSI-LD response format and join mode types.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Response format for entity queries (ETSI GS CIM 009 clause 4.5.1).
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
        matches!(self, Self::KeyValues | Self::Simplified)
    }
}

/// Join mode for linked entities (ETSI GS CIM 009 clause 4.5.21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum JoinMode {
    /// Linked entities are nested inside the referencing attribute.
    Inline,
    /// Linked entities are included as top-level objects in the notification.
    Flat,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
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
            let s = variant.to_string();
            let parsed = ResponseFormat::from_str(&s).unwrap();
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
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
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

    #[test]
    fn join_mode_display_and_parse_round_trip() {
        for variant in [JoinMode::Inline, JoinMode::Flat] {
            let s = variant.to_string();
            let parsed = JoinMode::from_str(&s).unwrap();
            pretty_assertions::assert_eq!(variant, parsed, "round-trip failed for {variant}");
        }
    }
}
