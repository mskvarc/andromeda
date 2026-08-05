//! The representation a temporal query response takes (ETSI GS CIM 009 clause 6.3.12).

use strum::{Display, EnumString};

/// Response format for temporal queries (clause 6.3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum TemporalFormat {
    /// Default normalized temporal representation.
    #[default]
    Normalized,
    /// Temporal values representation with compact arrays.
    TemporalValues,
    /// Aggregated values representation with statistical summaries.
    AggregatedValues,
}

#[cfg(test)]
mod tests {
    use crate::temporal_format::TemporalFormat;
    use std::str::FromStr;

    #[test]
    fn temporal_format_display_and_parse_round_trip() {
        for variant in [TemporalFormat::Normalized, TemporalFormat::TemporalValues, TemporalFormat::AggregatedValues] {
            let rendered = variant.to_string();
            let parsed = TemporalFormat::from_str(&rendered).expect("parse temporal format");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }
}
