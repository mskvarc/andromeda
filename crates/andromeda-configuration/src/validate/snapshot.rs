//! The `[snapshot]` rules.

use crate::validate::{
    bound::Relation,
    config_key::ConfigKey,
    rule::{Limit, Rule},
};

/// Both lifetimes are positive and the default is one a client could ask for.
/// Preparation needs positive waits and cadences, and a storage quota of no
/// bytes would refuse every Snapshot.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Positive(ConfigKey::SnapshotDefaultLifetime),
        Rule::Positive(ConfigKey::SnapshotMaxLifetime),
        Rule::Ordered {
            key: ConfigKey::SnapshotDefaultLifetime,
            relation: Relation::AtMost,
            limit: Limit::Key(ConfigKey::SnapshotMaxLifetime),
        },
        Rule::Positive(ConfigKey::SnapshotPreparationLeaseTimeout),
        Rule::Positive(ConfigKey::SnapshotPreparationRecoveryInterval),
        Rule::Positive(ConfigKey::SnapshotPreparationScopedOperationTimeout),
        Rule::Positive(ConfigKey::SnapshotMaxStoragePerTenant),
    ]
    .into_iter()
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        schema::ceiling::Ceiling,
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
        config.snapshot.quota.max_storage_per_tenant = Ceiling::At(HumanByteSize::b(0));
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::SnapshotMaxStoragePerTenant
            }]
        );
        config.snapshot.quota.max_storage_per_tenant = Ceiling::At(HumanByteSize::b(1));
        assert_accepted(&config);
    }
}
