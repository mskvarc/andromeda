//! The `[database]` rules.

use crate::{
    schema::database::{
        DatabaseConfig,
        name_cache::{IdleEviction, NameCacheConfig},
    },
    validate::{config_key::ConfigKey, violation_log::ViolationLog},
};

/// The pool needs a connection and a positive acquire deadline. A name cache
/// without capacity reads every name from its table, and a zero idle timeout
/// evicts every name the moment it is cached.
pub(crate) fn check_database(database: &DatabaseConfig, log: &mut ViolationLog) {
    log.positive_count(ConfigKey::DatabaseMaxConnections, database.max_connections.into());
    log.positive_duration(ConfigKey::DatabaseAcquireTimeout, database.acquire_timeout);
    check_name_cache(&database.name_cache, log);
}

/// The name cache's capacity and idle timeout.
fn check_name_cache(name_cache: &NameCacheConfig, log: &mut ViolationLog) {
    log.positive_count(ConfigKey::DatabaseNameCacheMaxSizePerDictionary, name_cache.max_size_per_dictionary.as_u64());
    match name_cache.idle_eviction_after {
        IdleEviction::After(idle) => log.positive_duration(ConfigKey::DatabaseNameCacheIdleEvictionAfter, idle),
        IdleEviction::Never(_) => {}
    }
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
