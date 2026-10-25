//! The time property a temporal query filters and orders on (ETSI GS CIM 009 clause 4.8).

use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// Temporal property to filter on (clause 4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumIter, EnumString, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum TimeProp {
    /// Select by observedAt timestamp (default).
    #[default]
    #[strum(serialize = "observedAt")]
    Observed,
    /// Select by createdAt timestamp.
    #[strum(serialize = "createdAt")]
    Created,
    /// Select by modifiedAt timestamp.
    #[strum(serialize = "modifiedAt")]
    Modified,
    /// Select by deletedAt timestamp.
    #[strum(serialize = "deletedAt")]
    Deleted,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::time_prop::TimeProp;
    use std::str::FromStr;

    #[test]
    fn time_prop_default_is_observed_at() {
        pretty_assertions::assert_eq!(TimeProp::default(), TimeProp::Observed);
    }

    #[test]
    fn time_prop_display_and_parse_round_trip() {
        for variant in [TimeProp::Observed, TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            let rendered = variant.to_string();
            let parsed = TimeProp::from_str(&rendered).expect("parse timeProperty");
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }
}
