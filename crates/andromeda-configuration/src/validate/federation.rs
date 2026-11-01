//! The `[federation]` rules.

use crate::{
    broker_config::BrokerConfig,
    schema::federation::cooldown::CooldownMode,
    validate::{
        bound::Relation,
        config_key::ConfigKey,
        rule::{CompoundRule, Limit, Rule},
        violation_log::ViolationLog,
    },
};

/// Connecting to a source and a forwarded request each need a positive
/// deadline. Every distinct entity a source answers is a row it sent, so a
/// row ceiling below the entity limit would refuse a query the entity limit
/// admits.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Positive(ConfigKey::FederationForwardingConnectTimeout),
        Rule::Positive(ConfigKey::FederationForwardingRequestTimeout),
        Rule::Ordered {
            key: ConfigKey::FederationForwardingMaxQuerySourceRows,
            relation: Relation::AtLeast,
            limit: Limit::Key(ConfigKey::FederationForwardingMaxQueryEntities),
        },
    ]
    .into_iter()
}

/// What [`check_cooldown`] asks.
pub(crate) const COMPOUND_RULES: [CompoundRule; 2] = [
    CompoundRule {
        keys: &[ConfigKey::FederationCooldownRetryInterval, ConfigKey::FederationCooldownMode],
        statement: "With `federation.cooldown.mode` other than `\"disabled\"`, `federation.cooldown.retry_interval` is above zero.",
    },
    CompoundRule {
        keys: &[ConfigKey::FederationCooldownMaxRetryInterval, ConfigKey::FederationCooldownMode],
        statement: "With `federation.cooldown.mode` at `\"exponential\"`, `federation.cooldown.max_retry_interval` is at least `federation.cooldown.retry_interval`.",
    },
];

/// A cooldown that holds sources back needs a hold to start from, and one
/// that doubles a cap it can start below. A disabled cooldown reads neither
/// key, and a constant one never grows to its cap.
pub(crate) fn check_cooldown(config: &BrokerConfig, log: &mut ViolationLog) {
    let positive_hold = Rule::Positive(ConfigKey::FederationCooldownRetryInterval);
    let reachable_cap = Rule::Ordered {
        key: ConfigKey::FederationCooldownMaxRetryInterval,
        relation: Relation::AtLeast,
        limit: Limit::Key(ConfigKey::FederationCooldownRetryInterval),
    };
    match config.federation.cooldown.mode {
        CooldownMode::Disabled => {}
        CooldownMode::Constant => log.apply(positive_hold, config),
        CooldownMode::Exponential => {
            log.apply(positive_hold, config);
            log.apply(reachable_cap, config);
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_duration::HumanDuration,
        schema::federation::cooldown::CooldownMode,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    #[test]
    fn a_zero_connect_or_request_timeout_is_refused() {
        assert_positive_duration(
            |config, value| config.federation.forwarding.connect_timeout = value,
            ConfigKey::FederationForwardingConnectTimeout,
        );
        assert_positive_duration(
            |config, value| config.federation.forwarding.request_timeout = value,
            ConfigKey::FederationForwardingRequestTimeout,
        );
    }

    #[test]
    fn a_source_row_ceiling_below_the_entity_limit_is_refused_and_an_equal_one_accepted() {
        let mut config = BrokerConfig::default();
        config.federation.forwarding.max_query_entities = NonZeroU32::new(500).expect("positive");
        config.federation.forwarding.max_query_source_rows = NonZeroU32::new(499).expect("positive");
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::FederationForwardingMaxQuerySourceRows,
                value: Quantity::Count(499),
                relation: Relation::AtLeast,
                bound: Bound::setting(ConfigKey::FederationForwardingMaxQueryEntities, 500_u64),
            }]
        );
        config.federation.forwarding.max_query_source_rows = NonZeroU32::new(500).expect("positive");
        assert_accepted(&config);
    }

    #[test]
    fn a_zero_cooldown_hold_is_refused_unless_the_cooldown_is_disabled() {
        let mut config = BrokerConfig::default();
        config.federation.cooldown.retry_interval = HumanDuration::from_secs(0);
        assert_accepted(&config);
        for mode in [CooldownMode::Constant, CooldownMode::Exponential] {
            config.federation.cooldown.mode = mode;
            assert_eq!(
                refusals(&config),
                [ConfigurationViolation::Zero {
                    key: ConfigKey::FederationCooldownRetryInterval
                }],
                "{mode}"
            );
        }
    }

    #[test]
    fn a_cooldown_cap_below_its_first_hold_is_refused_only_when_holds_grow() {
        let mut config = BrokerConfig::default();
        config.federation.cooldown.retry_interval = HumanDuration::from_secs(10);
        config.federation.cooldown.max_retry_interval = HumanDuration::from_secs(9);
        config.federation.cooldown.mode = CooldownMode::Constant;
        assert_accepted(&config);
        config.federation.cooldown.mode = CooldownMode::Exponential;
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::FederationCooldownMaxRetryInterval,
                value: Quantity::Duration(Duration::from_secs(9)),
                relation: Relation::AtLeast,
                bound: Bound::setting(ConfigKey::FederationCooldownRetryInterval, Duration::from_secs(10)),
            }]
        );
        config.federation.cooldown.max_retry_interval = HumanDuration::from_secs(10);
        assert_accepted(&config);
    }
}
