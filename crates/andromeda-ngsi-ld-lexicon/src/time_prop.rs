//! The time property a temporal query filters and orders on (ETSI GS CIM 009 clause 4.8).

use strum::{Display, EnumIter, EnumString, IntoStaticStr, VariantNames};

/// Temporal property to filter on (clause 4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumIter, EnumString, IntoStaticStr, VariantNames)]
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

impl TimeProp {
    /// Whether `name` is the name of one of the temporal properties, the
    /// system Attributes of clause 4.8, spelled exactly.
    #[must_use]
    pub fn names(name: &str) -> bool {
        TimeProp::VARIANTS.contains(&name)
    }
}

#[cfg(test)]
mod tests {
    use crate::time_prop::TimeProp;
    use std::str::FromStr;

    #[test]
    fn every_temporal_property_and_nothing_else_is_named() {
        for name in ["observedAt", "createdAt", "modifiedAt", "deletedAt"] {
            assert!(TimeProp::names(name), "{name}");
        }
        for name in ["location", "observedat", "ObservedAt", "id", ""] {
            assert!(!TimeProp::names(name), "{name}");
        }
    }

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
