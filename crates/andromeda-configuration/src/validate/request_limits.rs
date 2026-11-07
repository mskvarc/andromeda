//! The `[request_limits]` rules.

use crate::{
    header_section_limit::HeaderSectionLimit,
    schema::request_limits::{MAX_ATTRIBUTE_NESTING_CEILING, MAX_Q_NESTING_CEILING},
    validate::{
        bound::{Quantity, Relation},
        compound_rule::{CompoundRule, Obligation},
        config_key::ConfigKey,
        rule::{Limit, Rule},
    },
};

/// A request needs a positive deadline and a positive body ceiling, and a
/// collection page a positive one. A header
/// section limit outside its range is one some HTTP version cannot enforce
/// alike, and a nesting limit past its ceiling admits what no stored value or
/// parsed filter can hold.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Positive(ConfigKey::RequestLimitsTimeout),
        Rule::Positive(ConfigKey::RequestLimitsMaxBody),
        Rule::Positive(ConfigKey::RequestLimitsMaxPageBody),
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

/// A ceiling past the address width would admit every body or page.
pub(crate) const COMPOUND_RULES: [CompoundRule; 1] = [CompoundRule {
    statement: "It must fit the platform's address width.",
    when: &[],
    then: &[
        Obligation::FitsAddressWidth(ConfigKey::RequestLimitsMaxBody),
        Obligation::FitsAddressWidth(ConfigKey::RequestLimitsMaxPageBody),
    ],
}];

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        validate::{
            InvalidConfiguration,
            address_width::AddressWidth,
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, compound_refusals, refusals},
            validate_for,
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroUsize, time::Duration};

    #[test]
    fn a_nesting_limit_past_its_ceiling_is_refused_and_one_at_it_accepted() {
        let mut config = BrokerConfig::local();
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
        let mut config = BrokerConfig::local();
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
        let mut config = BrokerConfig::local();
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

    #[test]
    fn a_zero_page_body_ceiling_is_refused_and_one_byte_accepted() {
        let mut config = BrokerConfig::local();
        config.request_limits.max_page_body = HumanByteSize::b(0);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::RequestLimitsMaxPageBody
            }]
        );
        config.request_limits.max_page_body = HumanByteSize::b(1);
        assert_accepted(&config);
    }

    #[test]
    fn a_body_or_page_ceiling_past_a_32_bit_address_width_is_refused_and_one_within_accepted() {
        let mut config = BrokerConfig::local();
        config.request_limits.max_body = HumanByteSize::gib(5);
        config.request_limits.max_page_body = HumanByteSize::gib(5);
        assert_eq!(
            compound_refusals(&config, AddressWidth::new(32)),
            [
                ConfigurationViolation::ExceedsAddressWidth {
                    key: ConfigKey::RequestLimitsMaxBody,
                    bytes: 5 << 30,
                },
                ConfigurationViolation::ExceedsAddressWidth {
                    key: ConfigKey::RequestLimitsMaxPageBody,
                    bytes: 5 << 30,
                },
            ]
        );
        assert_eq!(compound_refusals(&config, AddressWidth::new(64)), []);
        config.request_limits.max_body = HumanByteSize::mib(64);
        config.request_limits.max_page_body = HumanByteSize::mib(64);
        assert_eq!(compound_refusals(&config, AddressWidth::new(32)), []);
    }

    /// No notification body limit or MQTT packet limit is above the largest
    /// byte size, so those rules refuse it.
    #[test]
    fn the_largest_body_ceiling_is_refused_by_the_notification_limits() {
        let mut config = BrokerConfig::local();
        config.request_limits.max_body = HumanByteSize::b(u64::MAX);
        assert_eq!(
            validate_for(&config, AddressWidth::new(64)).map_err(InvalidConfiguration::into_violations),
            Err(vec![
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::NotificationsDeliveryMaxHttpBody,
                    value: Quantity::Bytes(134_217_728),
                    relation: Relation::Above,
                    bound: Bound::setting(ConfigKey::RequestLimitsMaxBody, Quantity::Bytes(u64::MAX)),
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::NotificationsDeliveryMaxMqttPacket,
                    value: Quantity::Bytes(134_217_728),
                    relation: Relation::Above,
                    bound: Bound::setting(ConfigKey::RequestLimitsMaxBody, Quantity::Bytes(u64::MAX)),
                },
            ])
        );
    }
}
