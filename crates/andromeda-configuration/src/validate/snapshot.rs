//! The `[snapshot]` rules.

use crate::{
    schema::snapshot::SnapshotConfig,
    validate::{
        bound::{Bound, Relation},
        config_key::ConfigKey,
        violation_log::ViolationLog,
    },
};

/// Both lifetimes are positive and the default is one a client could ask for.
/// Preparation needs positive waits and cadences, and a storage quota of no
/// bytes would refuse every Snapshot.
pub(crate) fn check_snapshot(snapshot: &SnapshotConfig, log: &mut ViolationLog) {
    log.positive_duration(ConfigKey::SnapshotDefaultLifetime, snapshot.default_lifetime);
    log.positive_duration(ConfigKey::SnapshotMaxLifetime, snapshot.max_lifetime);
    log.require(
        ConfigKey::SnapshotDefaultLifetime,
        snapshot.default_lifetime.duration(),
        Relation::AtMost,
        Bound::setting(ConfigKey::SnapshotMaxLifetime, snapshot.max_lifetime.duration()),
    );
    let preparation = &snapshot.preparation;
    log.positive_duration(ConfigKey::SnapshotPreparationLeaseTimeout, preparation.lease_timeout);
    log.positive_duration(ConfigKey::SnapshotPreparationRecoveryInterval, preparation.recovery_interval);
    log.positive_duration(ConfigKey::SnapshotPreparationScopedOperationTimeout, preparation.scoped_operation_timeout);
    if let Some(storage) = snapshot.quota.max_storage_per_tenant {
        log.positive_count(ConfigKey::SnapshotMaxStoragePerTenant, storage.as_u64());
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
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn a_zero_default_lifetime_or_preparation_duration_is_refused() {
        assert_positive_duration(|config, value| config.snapshot.default_lifetime = value, ConfigKey::SnapshotDefaultLifetime);
        assert_positive_duration(
            |config, value| config.snapshot.preparation.lease_timeout = value,
            ConfigKey::SnapshotPreparationLeaseTimeout,
        );
        assert_positive_duration(
            |config, value| config.snapshot.preparation.recovery_interval = value,
            ConfigKey::SnapshotPreparationRecoveryInterval,
        );
        assert_positive_duration(
            |config, value| config.snapshot.preparation.scoped_operation_timeout = value,
            ConfigKey::SnapshotPreparationScopedOperationTimeout,
        );
    }

    /// A zero maximum is also below the default it must not undercut.
    #[test]
    fn a_zero_max_lifetime_is_refused_with_the_default_it_must_hold() {
        let mut config = BrokerConfig::default();
        config.snapshot.max_lifetime = HumanDuration::from_secs(0);
        assert_eq!(
            refusals(&config),
            [
                ConfigurationViolation::Zero {
                    key: ConfigKey::SnapshotMaxLifetime
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::SnapshotDefaultLifetime,
                    value: Quantity::Duration(Duration::from_secs(3_600)),
                    relation: Relation::AtMost,
                    bound: Bound::setting(ConfigKey::SnapshotMaxLifetime, Duration::ZERO),
                },
            ]
        );
    }

    #[test]
    fn a_default_lifetime_past_the_maximum_is_refused_and_an_equal_one_accepted() {
        let mut config = BrokerConfig::default();
        config.snapshot.max_lifetime = HumanDuration::from_secs(3_600);
        config.snapshot.default_lifetime = HumanDuration::from_millis(3_600_001);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::SnapshotDefaultLifetime,
                value: Quantity::Duration(Duration::from_millis(3_600_001)),
                relation: Relation::AtMost,
                bound: Bound::setting(ConfigKey::SnapshotMaxLifetime, Duration::from_secs(3_600)),
            }]
        );
        config.snapshot.default_lifetime = HumanDuration::from_secs(3_600);
        assert_accepted(&config);
    }

    #[test]
    fn a_storage_quota_of_no_bytes_is_refused_and_one_byte_accepted() {
        let mut config = BrokerConfig::default();
        config.snapshot.quota.max_storage_per_tenant = Some(HumanByteSize::b(0));
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::SnapshotMaxStoragePerTenant
            }]
        );
        config.snapshot.quota.max_storage_per_tenant = Some(HumanByteSize::b(1));
        assert_accepted(&config);
    }
}
