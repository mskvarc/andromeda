//! What an ordering rule compares a configured value against.

use crate::validate::config_key::ConfigKey;
use derive_more::{Display, From};
use humantime::format_duration;
use std::time::Duration;

/// A configured value an ordering rule compares: a span of time or a count.
///
/// A rule compares a value only with a bound of the same kind, so the derived
/// order, which puts every duration before every count, never decides one.
#[derive(Clone, Copy, Debug, Display, Eq, From, Ord, PartialEq, PartialOrd)]
pub enum Quantity {
    /// A span of time, displayed in its `humantime` spelling.
    #[display("{}", format_duration(*_0))]
    Duration(Duration),
    /// A number of items or bytes.
    #[display("{_0}")]
    Count(u64),
}

/// How a configured value must relate to its bound.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Relation {
    /// Strictly less than the bound.
    #[display("below")]
    Below,
    /// Strictly greater than the bound.
    #[display("above")]
    Above,
    /// Greater than or equal to the bound.
    #[display("at least")]
    AtLeast,
    /// Less than or equal to the bound.
    #[display("at most")]
    AtMost,
}

impl Relation {
    /// Whether `value` stands in this relation to `limit`.
    #[must_use]
    pub fn holds(self, value: Quantity, limit: Quantity) -> bool {
        match self {
            Relation::Below => value < limit,
            Relation::Above => value > limit,
            Relation::AtLeast => value >= limit,
            Relation::AtMost => value <= limit,
        }
    }
}

/// What a configured value is compared against.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Bound {
    /// A limit the broker fixes.
    #[display("{_0}")]
    Fixed(Quantity),
    /// Another configured key and the value it holds.
    #[display("{key} ({value})")]
    Setting {
        /// The key the bound is read from.
        key: ConfigKey,
        /// The value that key holds.
        value: Quantity,
    },
    /// Another configured duration with a margin the broker fixes on top.
    #[display("{key} ({}) plus {}", format_duration(*value), format_duration(*margin))]
    SettingPlusMargin {
        /// The key the bound is read from.
        key: ConfigKey,
        /// The duration that key holds.
        value: Duration,
        /// The margin added to it.
        margin: Duration,
    },
}

impl Bound {
    /// A bound read from `key`, which holds `value`.
    #[must_use]
    pub fn setting(key: ConfigKey, value: impl Into<Quantity>) -> Bound {
        Bound::Setting { key, value: value.into() }
    }

    /// The value compared against.
    #[must_use]
    pub const fn limit(self) -> Quantity {
        match self {
            Bound::Fixed(limit) | Bound::Setting { value: limit, .. } => limit,
            Bound::SettingPlusMargin { value, margin, .. } => Quantity::Duration(value.saturating_add(margin)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::validate::{
        bound::{Bound, Quantity, Relation},
        config_key::ConfigKey,
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn each_relation_holds_on_its_side_of_the_limit() {
        let (less, limit, more) = (Quantity::Count(1), Quantity::Count(2), Quantity::Count(3));
        assert_eq!([less, limit, more].map(|value| Relation::Below.holds(value, limit)), [true, false, false]);
        assert_eq!([less, limit, more].map(|value| Relation::Above.holds(value, limit)), [false, false, true]);
        assert_eq!([less, limit, more].map(|value| Relation::AtLeast.holds(value, limit)), [false, true, true]);
        assert_eq!([less, limit, more].map(|value| Relation::AtMost.holds(value, limit)), [true, true, false]);
    }

    #[test]
    fn a_margin_bound_limits_at_the_setting_plus_the_margin() {
        let bound = Bound::SettingPlusMargin {
            key: ConfigKey::DatabaseAcquireTimeout,
            value: Duration::from_secs(5),
            margin: Duration::from_secs(5),
        };
        assert_eq!(bound.limit(), Quantity::Duration(Duration::from_secs(10)));
        assert_eq!(bound.to_string(), "database.acquire_timeout (5s) plus 5s");
    }

    #[test]
    fn a_setting_bound_names_its_key_and_value() {
        assert_eq!(
            Bound::setting(ConfigKey::RequestLimitsTimeout, Duration::from_secs(30)).to_string(),
            "request_limits.timeout (30s)"
        );
    }
}
