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
    /// Grouped aggregated representation: the aggregated values further keyed by
    /// one or more group values (`aggrGroupBy`). Non-standard; served only when
    /// the temporal aggregation grouping extension is enabled.
    GroupedValues,
}

impl TemporalFormat {
    /// Whether the format applies the clause 4.5.19 aggregation methods.
    ///
    /// Both [`TemporalFormat::AggregatedValues`] and the grouped extension
    /// [`TemporalFormat::GroupedValues`] run the aggregation pipeline; the two
    /// instance-returning formats do not. Centralised so every code path that
    /// asks "is this an aggregation?" agrees, since the grouped format must be
    /// treated as one everywhere the aggregation pipeline branches.
    #[must_use]
    pub const fn is_aggregated(&self) -> bool {
        matches!(self, TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues)
    }

    /// Whether the format nests the aggregated values under group-key values.
    ///
    /// True only for the [`TemporalFormat::GroupedValues`] extension format,
    /// which is the sole point where the aggregation pipeline and the plain
    /// aggregated format diverge.
    #[must_use]
    pub const fn is_grouped(&self) -> bool {
        matches!(self, TemporalFormat::GroupedValues)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal_format::TemporalFormat;
    use std::str::FromStr;

    #[test]
    fn temporal_format_display_and_parse_round_trip() {
        for variant in [
            TemporalFormat::Normalized,
            TemporalFormat::TemporalValues,
            TemporalFormat::AggregatedValues,
            TemporalFormat::GroupedValues,
        ] {
            let rendered = variant.to_string();
            let parsed = TemporalFormat::from_str(&rendered).expect("parse temporal format");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }
}
