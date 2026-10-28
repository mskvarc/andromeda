//! The `[federation]` rules. The cooldown schedule is checked by the
//! forwarding layer that builds it.

use crate::validate::{
    bound::Relation,
    config_key::ConfigKey,
    rule::{Limit, Rule},
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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

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
}
