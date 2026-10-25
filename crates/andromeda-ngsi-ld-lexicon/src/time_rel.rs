//! The temporal relationship a temporal query filters by (ETSI GS CIM 009 clause 4.11).

use strum::{Display, EnumString};

/// Temporal relationship for time-based filtering (clause 4.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum TimeRel {
    /// time < timeAt (exclusive upper bound).
    Before,
    /// time >= timeAt (inclusive lower bound).
    After,
    /// time >= timeAt AND time < endTimeAt (inclusive start, exclusive end).
    Between,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::time_rel::TimeRel;
    use std::str::FromStr;

    #[test]
    fn time_rel_display_and_parse_round_trip() {
        for variant in [TimeRel::Before, TimeRel::After, TimeRel::Between] {
            let rendered = variant.to_string();
            let parsed = TimeRel::from_str(&rendered).expect("parse timeRel");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }
}
