//! The `[request_limits]` rules.

use crate::{
    schema::request_limits::RequestLimitsConfig,
    validate::{
        config_key::ConfigKey,
        rule::{CompoundRule, Rule},
        violation::ConfigurationViolation,
        violation_log::ViolationLog,
    },
};

/// A request needs a positive deadline and a positive body ceiling.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [Rule::Positive(ConfigKey::RequestLimitsTimeout), Rule::Positive(ConfigKey::RequestLimitsMaxBody)].into_iter()
}

/// What [`check_request_limits`] asks.
pub(crate) const COMPOUND_RULES: [CompoundRule; 1] = [CompoundRule {
    keys: &[ConfigKey::RequestLimitsMaxBody],
    statement: "It must fit the platform's address width.",
}];

/// A ceiling past the address width would admit every body.
pub(crate) fn check_request_limits(limits: &RequestLimitsConfig, log: &mut ViolationLog) {
    if limits.max_body_bytes().is_err() {
        log.record(ConfigurationViolation::ExceedsAddressWidth {
            key: ConfigKey::RequestLimitsMaxBody,
            bytes: limits.max_body.as_u64(),
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// A zero deadline is also not above either deadline that must stay
    /// below it.
    #[test]
    fn a_zero_request_timeout_is_refused_with_the_deadlines_it_must_exceed() {
        let mut config = BrokerConfig::default();
        config.request_limits.timeout = HumanDuration::from_secs(0);
        let zero = Bound::setting(ConfigKey::RequestLimitsTimeout, Duration::ZERO);
        assert_eq!(
            refusals(&config),
            [
                ConfigurationViolation::Zero {
                    key: ConfigKey::RequestLimitsTimeout
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::DatabaseAcquireTimeout,
                    value: Quantity::Duration(Duration::from_secs(5)),
                    relation: Relation::Below,
                    bound: zero,
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::TenancyBuildAdmissionTimeout,
                    value: Quantity::Duration(Duration::from_secs(20)),
                    relation: Relation::Below,
                    bound: zero,
                },
            ]
        );
        config.request_limits.timeout = HumanDuration::from_millis(20_001);
        assert_accepted(&config);
    }

    #[test]
    fn a_zero_body_ceiling_is_refused_and_one_byte_accepted() {
        let mut config = BrokerConfig::default();
        config.request_limits.max_body = HumanByteSize::b(0);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::RequestLimitsMaxBody
            }]
        );
        config.request_limits.max_body = HumanByteSize::b(1);
        assert_accepted(&config);
    }

    /// The address width is checked only where `usize` is narrower than
    /// `u64`, the only platforms a byte size can pass it on.
    #[test]
    fn a_body_ceiling_past_the_address_width_is_refused() {
        let mut config = BrokerConfig::default();
        config.request_limits.max_body = HumanByteSize::b(u64::MAX);
        if usize::try_from(u64::MAX).is_ok() {
            assert_accepted(&config);
        } else {
            assert_eq!(
                refusals(&config),
                [ConfigurationViolation::ExceedsAddressWidth {
                    key: ConfigKey::RequestLimitsMaxBody,
                    bytes: u64::MAX,
                }]
            );
        }
    }
}
