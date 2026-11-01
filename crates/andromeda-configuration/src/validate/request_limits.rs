//! The `[request_limits]` rules.

use crate::{
    header_section_limit::HeaderSectionLimit,
    schema::request_limits::{MAX_ATTRIBUTE_NESTING_CEILING, MAX_Q_NESTING_CEILING, RequestLimitsConfig},
    validate::{
        bound::{Quantity, Relation},
        config_key::ConfigKey,
        rule::{CompoundRule, Limit, Rule},
        violation::ConfigurationViolation,
        violation_log::ViolationLog,
    },
};

/// A request needs a positive deadline and a positive body ceiling. A header
/// section limit outside its range is one some HTTP version cannot enforce
/// alike, and a nesting limit past its ceiling admits what no stored value or
/// parsed filter can hold.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Positive(ConfigKey::RequestLimitsTimeout),
        Rule::Positive(ConfigKey::RequestLimitsMaxBody),
        Rule::Ordered {
            key: ConfigKey::RequestLimitsMaxHeaderSection,
            relation: Relation::AtLeast,
            limit: Limit::Fixed(Quantity::Bytes(u64::from(HeaderSectionLimit::MINIMUM.octets()))),
        },
        Rule::Ordered {
            key: ConfigKey::RequestLimitsMaxHeaderSection,
            relation: Relation::AtMost,
            limit: Limit::Fixed(Quantity::Bytes(u64::from(HeaderSectionLimit::MAXIMUM.octets()))),
        },
        ceiling(ConfigKey::RequestLimitsMaxAttributeNesting, MAX_ATTRIBUTE_NESTING_CEILING),
        ceiling(ConfigKey::RequestLimitsMaxQNesting, MAX_Q_NESTING_CEILING),
    ]
    .into_iter()
}

/// `key` holds at most the count `ceiling`.
fn ceiling(key: ConfigKey, ceiling: usize) -> Rule {
    Rule::Ordered {
        key,
        relation: Relation::AtMost,
        limit: Limit::Fixed(Quantity::Count(u64::try_from(ceiling).unwrap_or(u64::MAX))),
    }
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
    use std::{num::NonZeroUsize, time::Duration};

    #[test]
    fn a_nesting_limit_past_its_ceiling_is_refused_and_one_at_it_accepted() {
        let mut config = BrokerConfig::default();
        config.request_limits.max_attribute_nesting = NonZeroUsize::new(63).expect("a positive depth");
        config.request_limits.max_q_nesting = NonZeroUsize::new(65).expect("a positive depth");
        assert_eq!(
            refusals(&config),
            [
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::RequestLimitsMaxAttributeNesting,
                    value: Quantity::Count(63),
                    relation: Relation::AtMost,
                    bound: Bound::Fixed(Quantity::Count(62)),
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::RequestLimitsMaxQNesting,
                    value: Quantity::Count(65),
                    relation: Relation::AtMost,
                    bound: Bound::Fixed(Quantity::Count(64)),
                },
            ]
        );
        config.request_limits.max_attribute_nesting = NonZeroUsize::new(62).expect("a positive depth");
        config.request_limits.max_q_nesting = NonZeroUsize::new(64).expect("a positive depth");
        assert_accepted(&config);
    }

    /// A zero deadline is also not above any deadline that must stay below
    /// it.
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
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::SnapshotPreparationScopedOperationTimeout,
                    value: Quantity::Duration(Duration::from_secs(20)),
                    relation: Relation::Below,
                    bound: zero,
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::JsonLdFetchTimeout,
                    value: Quantity::Duration(Duration::from_secs(10)),
                    relation: Relation::Below,
                    bound: zero,
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::FederationForwardingRequestTimeout,
                    value: Quantity::Duration(Duration::from_secs(5)),
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
    /// `u64`, the only platforms a byte size can pass it on. No notification
    /// body limit is above the largest byte size, so that rule refuses too.
    #[test]
    fn a_body_ceiling_past_the_address_width_is_refused() {
        let mut config = BrokerConfig::default();
        config.request_limits.max_body = HumanByteSize::b(u64::MAX);
        let notification_limit = ConfigurationViolation::OutOfRange {
            key: ConfigKey::NotificationsDeliveryMaxHttpBody,
            value: Quantity::Bytes(134_217_728),
            relation: Relation::Above,
            bound: Bound::setting(ConfigKey::RequestLimitsMaxBody, Quantity::Bytes(u64::MAX)),
        };
        if usize::try_from(u64::MAX).is_ok() {
            assert_eq!(refusals(&config), [notification_limit]);
        } else {
            assert_eq!(
                refusals(&config),
                [
                    ConfigurationViolation::ExceedsAddressWidth {
                        key: ConfigKey::RequestLimitsMaxBody,
                        bytes: u64::MAX,
                    },
                    notification_limit,
                ]
            );
        }
    }
}
