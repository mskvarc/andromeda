//! The `[database]` rules.

use crate::{
    schema::database::write_reserve::WriteReserve,
    validate::{
        bound::{Quantity, Relation},
        config_key::ConfigKey,
        rule::{Limit, Rule},
    },
};

/// The pool needs a connection and a positive acquire deadline, and a write
/// reserve that leaves reads one. A name cache without capacity reads every
/// name from its table, and a zero idle timeout evicts every name the moment
/// it is cached.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Positive(ConfigKey::DatabaseMaxConnections),
        Rule::Positive(ConfigKey::DatabaseAcquireTimeout),
        Rule::Ordered {
            key: ConfigKey::DatabaseWriteReservePercent,
            relation: Relation::Below,
            limit: Limit::Fixed(Quantity::Count(u64::from(WriteReserve::CEILING))),
        },
        Rule::Positive(ConfigKey::DatabaseNameCacheMaxSizePerDictionary),
        Rule::Positive(ConfigKey::DatabaseNameCacheIdleEvictionAfter),
    ]
    .into_iter()
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        schema::database::name_cache::{IdleEviction, Unlimited},
        validate::{
            config_key::ConfigKey,
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_pool_without_connections_is_refused_and_one_connection_accepted() {
        let mut config = BrokerConfig::default();
        config.database.max_connections = 0;
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::DatabaseMaxConnections
            }]
        );
        config.database.max_connections = 1;
        assert_accepted(&config);
    }

    #[test]
    fn a_zero_acquire_timeout_is_refused() {
        assert_positive_duration(|config, value| config.database.acquire_timeout = value, ConfigKey::DatabaseAcquireTimeout);
    }

    #[test]
    fn a_name_cache_without_capacity_is_refused_and_one_byte_accepted() {
        let mut config = BrokerConfig::default();
        config.database.name_cache.max_size_per_dictionary = HumanByteSize::b(0);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::DatabaseNameCacheMaxSizePerDictionary
            }]
        );
        config.database.name_cache.max_size_per_dictionary = HumanByteSize::b(1);
        assert_accepted(&config);
    }

    #[test]
    fn a_zero_name_idle_timeout_is_refused_and_unlimited_accepted() {
        assert_positive_duration(
            |config, value| config.database.name_cache.idle_eviction_after = IdleEviction::After(value),
            ConfigKey::DatabaseNameCacheIdleEvictionAfter,
        );
        let mut config = BrokerConfig::default();
        config.database.name_cache.idle_eviction_after = IdleEviction::Never(Unlimited::Unlimited);
        assert_accepted(&config);
    }
}
