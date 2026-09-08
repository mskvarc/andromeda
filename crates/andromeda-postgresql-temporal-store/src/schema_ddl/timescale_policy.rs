//! The columnstore-policy procedures of the temporal hypertables.

use std::num::NonZeroU32;
use strum::{Display, EnumIter, IntoStaticStr};

/// The hypertables of the temporal schema whose storage policy the broker
/// reconciles.
///
/// The name renders as the catalogue relation name, which is what every
/// `TimescaleDB` procedure and information view takes.
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum HypertableName {
    /// `attribute_history`, segmented by stream.
    AttributeHistory,
    /// `scope_history`, segmented by temporal entity.
    ScopeHistory,
}

/// `CALL add_columnstore_policy(hypertable, after => INTERVAL 'n days', if_not_exists => true)`.
///
/// The lag is rendered as an interval literal: it is a validated positive day
/// count from configuration, not caller data, and a procedure argument cannot
/// be a bound parameter of a `CALL` executed through this driver.
#[must_use]
pub fn add_columnstore_policy(hypertable: HypertableName, after_days: NonZeroU32) -> String {
    format!("CALL add_columnstore_policy('{hypertable}', after => INTERVAL '{after_days} days', if_not_exists => true)")
}

/// `CALL remove_columnstore_policy(hypertable, if_exists => true)`.
#[must_use]
pub fn remove_columnstore_policy(hypertable: HypertableName) -> String {
    format!("CALL remove_columnstore_policy('{hypertable}', if_exists => true)")
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::timescale_policy::{HypertableName, add_columnstore_policy, remove_columnstore_policy};
    use std::num::NonZeroU32;

    #[test]
    fn the_hypertable_renders_as_its_catalogue_name() {
        assert_eq!(HypertableName::AttributeHistory.to_string(), "attribute_history");
        assert_eq!(HypertableName::ScopeHistory.to_string(), "scope_history");
    }

    #[test]
    fn the_policy_is_added_idempotently_with_a_day_interval() {
        assert_eq!(
            add_columnstore_policy(HypertableName::AttributeHistory, NonZeroU32::new(3).expect("positive")),
            "CALL add_columnstore_policy('attribute_history', after => INTERVAL '3 days', if_not_exists => true)"
        );
    }

    #[test]
    fn the_policy_is_removed_idempotently() {
        assert_eq!(
            remove_columnstore_policy(HypertableName::ScopeHistory),
            "CALL remove_columnstore_policy('scope_history', if_exists => true)"
        );
    }
}
