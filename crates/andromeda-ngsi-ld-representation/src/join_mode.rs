//! How a linked entity reaches the response (clause 4.5.21).

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

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
    use crate::join_mode::JoinMode;
    use std::str::FromStr;

    #[test]
    fn join_mode_display_and_parse_round_trip() {
        for variant in [JoinMode::Inline, JoinMode::Flat] {
            let rendered = variant.to_string();
            let parsed = JoinMode::from_str(&rendered).expect("round trip");
            pretty_assertions::assert_eq!(variant, parsed, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn join_mode_serde_round_trip() {
        for variant in [JoinMode::Inline, JoinMode::Flat] {
            let json = serde_json::to_string(&variant).expect("serialises");
            let parsed: JoinMode = serde_json::from_str(&json).expect("deserialises");
            pretty_assertions::assert_eq!(variant, parsed);
        }
    }
}
