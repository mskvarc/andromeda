//! The comparisons a configured value obeys, held as data.
//!
//! A rule that compares one key with zero, with a fixed limit or with another
//! key is a [`Rule`]: the validator evaluates it against a configuration, and
//! the configuration descriptor lists it under its key, from the one
//! declaration.

use crate::{
    human_duration::HumanDuration,
    validate::{
        bound::{Quantity, Relation},
        config_key::ConfigKey,
    },
};
use derive_more::Display;
use std::time::Duration;

/// One comparison a configured value must pass, displayed as the condition
/// it states.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Rule {
    /// The key holds more than zero.
    #[display("above zero")]
    Positive(ConfigKey),
    /// The key stands in `relation` to `limit`.
    #[display("{relation} {limit}")]
    Ordered {
        /// The key compared.
        key: ConfigKey,
        /// How it must relate to the limit.
        relation: Relation,
        /// What it is compared with.
        limit: Limit,
    },
    /// The key holds a positive whole number of days.
    #[display("a whole number of days")]
    WholeDays(ConfigKey),
}

impl Rule {
    /// The key the rule constrains.
    #[must_use]
    pub const fn key(self) -> ConfigKey {
        match self {
            Rule::Positive(key) | Rule::Ordered { key, .. } | Rule::WholeDays(key) => key,
        }
    }
}

/// What an ordering rule compares a key with, before a configuration gives
/// it a value.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Limit {
    /// A limit the broker fixes.
    #[display("{_0}")]
    Fixed(Quantity),
    /// The value another key holds.
    #[display("`{_0}`")]
    Key(ConfigKey),
    /// The duration another key holds, plus a margin the broker fixes.
    #[display("`{_0}` plus {}", HumanDuration::new(*_1))]
    KeyPlusMargin(ConfigKey, Duration),
}

#[cfg(test)]
mod tests {
    use crate::validate::{
        bound::{Quantity, Relation},
        config_key::{ConfigKey, QueueKey, QueueName},
        rule::{Limit, Rule},
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn each_rule_reads_as_the_condition_it_states() {
        let rules = [
            Rule::Positive(ConfigKey::ServerPort),
            Rule::WholeDays(ConfigKey::TemporalStorageChunkInterval),
            Rule::Ordered {
                key: ConfigKey::TemporalQueryMaxLastN,
                relation: Relation::AtMost,
                limit: Limit::Fixed(Quantity::Count(5000)),
            },
            Rule::Ordered {
                key: ConfigKey::ServerAnnouncedCloseTimeout,
                relation: Relation::Below,
                limit: Limit::Key(ConfigKey::ServerGracefulDrainTimeout),
            },
            Rule::Ordered {
                key: ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::LeaseTimeout),
                relation: Relation::AtLeast,
                limit: Limit::KeyPlusMargin(ConfigKey::NotificationsDeliveryRequestTimeout, Duration::from_secs(5)),
            },
        ];
        assert_eq!(
            rules.map(|rule| rule.to_string()),
            [
                "above zero",
                "a whole number of days",
                "at most 5000",
                "below `server.graceful_drain_timeout`",
                "at least `notifications.delivery.request_timeout` plus 5s",
            ]
        );
    }
}
