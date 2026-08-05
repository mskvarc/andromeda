//! The aggregation methods a temporal query may request (ETSI GS CIM 009 clause 4.5.19).

use strum::{Display, EnumString};

/// Aggregation methods for temporal queries (clause 4.5.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum AggrMethod {
    /// Arithmetic mean.
    Avg,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Sum of numeric values.
    Sum,
    /// Sum of squares.
    #[strum(serialize = "sumsq")]
    SumSq,
    /// Standard deviation.
    #[strum(serialize = "stddev")]
    StdDev,
    /// Total count of values.
    TotalCount,
    /// Count of distinct values.
    DistinctCount,
}

#[cfg(test)]
mod tests {
    use crate::aggr_method::AggrMethod;
    use std::str::FromStr;

    #[test]
    fn aggr_method_display_and_parse_round_trip() {
        for variant in [
            AggrMethod::Avg,
            AggrMethod::Min,
            AggrMethod::Max,
            AggrMethod::Sum,
            AggrMethod::SumSq,
            AggrMethod::StdDev,
            AggrMethod::TotalCount,
            AggrMethod::DistinctCount,
        ] {
            let rendered = variant.to_string();
            let parsed = AggrMethod::from_str(&rendered).expect("parse aggrMethod");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }
}
