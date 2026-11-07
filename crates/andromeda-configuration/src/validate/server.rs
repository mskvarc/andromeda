//! The `[server]` rules.

use crate::validate::{
    bound::Relation,
    config_key::ConfigKey,
    rule::{Limit, Rule},
};

/// Port zero is reserved for dynamic binding and cannot be advertised. A zero
/// announced-close timeout resets a client still sending before it reads the
/// response, and a connection delaying its close must never outlive the drain.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Positive(ConfigKey::ServerPort),
        Rule::Positive(ConfigKey::ServerAnnouncedCloseTimeout),
        Rule::Ordered {
            key: ConfigKey::ServerAnnouncedCloseTimeout,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::ServerGracefulDrainTimeout),
        },
    ]
    .into_iter()
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_duration::HumanDuration,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn port_zero_is_refused_and_port_one_accepted() {
        let mut config = BrokerConfig::local();
        config.server.port = 0;
        assert_eq!(refusals(&config), [ConfigurationViolation::Zero { key: ConfigKey::ServerPort }]);
        config.server.port = 1;
        assert_accepted(&config);
    }

    #[test]
    fn a_zero_announced_close_timeout_is_refused() {
        assert_positive_duration(
            |config, value| config.server.announced_close_timeout = value,
            ConfigKey::ServerAnnouncedCloseTimeout,
        );
    }

    #[test]
    fn an_announced_close_timeout_at_the_graceful_drain_is_refused_and_one_step_below_accepted() {
        let mut config = BrokerConfig::local();
        config.server.graceful_drain_timeout = HumanDuration::from_secs(5);
        config.server.announced_close_timeout = HumanDuration::from_secs(5);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::ServerAnnouncedCloseTimeout,
                value: Quantity::Duration(Duration::from_secs(5)),
                relation: Relation::Below,
                bound: Bound::setting(ConfigKey::ServerGracefulDrainTimeout, Duration::from_secs(5)),
            }]
        );
        config.server.announced_close_timeout = HumanDuration::from_millis(4_999);
        assert_accepted(&config);
    }
}
