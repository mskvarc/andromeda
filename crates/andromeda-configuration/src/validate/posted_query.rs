//! The `[posted_query]` rules.

use crate::validate::{config_key::ConfigKey, rule::Rule};

/// A query kept for no time would leave every page link it is named by
/// refused. The storage quota is bounded against the largest body a request
/// may send among the rules spanning tables.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [Rule::Positive(ConfigKey::PostedQueryLifetime)].into_iter()
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        schema::ceiling::{Ceiling, Unlimited},
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_zero_lifetime_is_refused() {
        assert_positive_duration(|config, value| config.posted_query.lifetime = value, ConfigKey::PostedQueryLifetime);
    }

    /// A query a client may send must fit the quota on its own, or the POST
    /// that stores it would leave its tenant over the quota at once.
    #[test]
    fn a_storage_quota_below_the_largest_body_is_refused_and_an_equal_one_accepted() {
        let mut config = BrokerConfig::local();
        config.posted_query.quota.max_storage_per_tenant = Ceiling::At(HumanByteSize::b(67_108_863));
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::PostedQueryMaxStoragePerTenant,
                value: Quantity::Bytes(67_108_863),
                relation: Relation::AtLeast,
                bound: Bound::setting(ConfigKey::RequestLimitsMaxBody, Quantity::Bytes(67_108_864)),
            }]
        );
        config.posted_query.quota.max_storage_per_tenant = Ceiling::At(HumanByteSize::mib(64));
        assert_accepted(&config);
    }

    #[test]
    fn an_unlimited_storage_quota_is_accepted() {
        let mut config = BrokerConfig::local();
        config.posted_query.quota.max_storage_per_tenant = Ceiling::Unlimited(Unlimited::Unlimited);
        assert_accepted(&config);
    }
}
