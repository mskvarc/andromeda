//! The `[snapshot]` tables: how long a Snapshot lives, how many a tenant may
//! keep, and how one is prepared.

use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid Snapshot configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SnapshotConfigError {
    /// The default lifetime must be positive.
    #[error("snapshot.default_lifetime must be greater than zero")]
    ZeroDefaultLifetime,
    /// The maximum lifetime must be positive.
    #[error("snapshot.max_lifetime must be greater than zero")]
    ZeroMaxLifetime,
    /// The default lifetime cannot exceed the maximum.
    #[error("snapshot.default_lifetime must not exceed snapshot.max_lifetime")]
    DefaultLifetimeExceedsMax,
    /// Scoped operations need a positive bounded wait.
    #[error("snapshot.preparation.scoped_operation_timeout must be greater than zero")]
    ZeroScopedOperationTimeout,
    /// Preparation claims need a positive lease.
    #[error("snapshot.preparation.lease_timeout must be greater than zero")]
    ZeroLeaseTimeout,
    /// The recovery sweep would run without pause.
    #[error("snapshot.preparation.recovery_interval must be greater than zero")]
    ZeroRecoveryInterval,
    /// A storage quota of no bytes would refuse every Snapshot.
    #[error("snapshot.quota.max_storage_per_tenant must be at least 1 byte")]
    ZeroStorageQuota,
}

/// How many Snapshots one tenant may keep. Each key left out is no limit.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotQuotaConfig {
    /// The most Snapshots one tenant keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_tenant: Option<NonZeroUsize>,
    /// The most storage one tenant's Snapshots hold, in human-readable form
    /// (`"10 GiB"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_storage_per_tenant: Option<HumanByteSize>,
}

/// How a Snapshot is prepared.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreparationConfig {
    /// The results read in each capture page.
    #[toml_comment(inline)]
    pub page_size: NonZeroUsize,
    /// The most Snapshots prepared at once.
    #[toml_comment(inline)]
    pub max_concurrent: NonZeroUsize,
    /// How long a claim keeps a preparation from every other worker, in
    /// human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub lease_timeout: HumanDuration,
    /// How often preparations a crash or a missed run left unfinished are
    /// resumed, in human-readable form (`"30s"`). Creation prepares its
    /// Snapshot at once, so this sweep only recovers.
    #[toml_comment(inline)]
    pub recovery_interval: HumanDuration,
    /// How long a scoped operation waits for capture or clone promotion, in
    /// human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub scoped_operation_timeout: HumanDuration,
}

impl Default for PreparationConfig {
    fn default() -> PreparationConfig {
        PreparationConfig {
            page_size: NonZeroUsize::MIN.saturating_add(249),
            max_concurrent: NonZeroUsize::MIN.saturating_add(3),
            lease_timeout: HumanDuration::from_secs(30),
            recovery_interval: HumanDuration::from_secs(30),
            scoped_operation_timeout: HumanDuration::from_secs(30),
        }
    }
}

/// How long a Snapshot lives, how many a tenant may keep, and how one is
/// prepared.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotConfig {
    /// The lifetime of a Snapshot created without `snapshotLifetime`, in
    /// human-readable form (`"1h"`).
    #[toml_comment(inline)]
    pub default_lifetime: HumanDuration,
    /// The longest `snapshotLifetime` accepted, in human-readable form
    /// (`"30days"`).
    #[toml_comment(inline)]
    pub max_lifetime: HumanDuration,
    /// How many Snapshots one tenant may keep.
    pub quota: SnapshotQuotaConfig,
    /// How a Snapshot is prepared.
    pub preparation: PreparationConfig,
}

impl Default for SnapshotConfig {
    fn default() -> SnapshotConfig {
        SnapshotConfig {
            default_lifetime: HumanDuration::from_secs(3_600),
            max_lifetime: HumanDuration::from_days(30),
            quota: SnapshotQuotaConfig::default(),
            preparation: PreparationConfig::default(),
        }
    }
}

impl SnapshotConfig {
    /// Validate the lifetimes, the preparation durations and the storage
    /// quota.
    ///
    /// # Errors
    ///
    /// Returns the [`SnapshotConfigError`] naming the first value refused.
    pub fn validate(&self) -> Result<(), SnapshotConfigError> {
        if self.default_lifetime.is_zero() {
            Err(SnapshotConfigError::ZeroDefaultLifetime)
        } else if self.max_lifetime.is_zero() {
            Err(SnapshotConfigError::ZeroMaxLifetime)
        } else if self.default_lifetime.duration() > self.max_lifetime.duration() {
            Err(SnapshotConfigError::DefaultLifetimeExceedsMax)
        } else if self.preparation.scoped_operation_timeout.is_zero() {
            Err(SnapshotConfigError::ZeroScopedOperationTimeout)
        } else if self.preparation.lease_timeout.is_zero() {
            Err(SnapshotConfigError::ZeroLeaseTimeout)
        } else if self.preparation.recovery_interval.is_zero() {
            Err(SnapshotConfigError::ZeroRecoveryInterval)
        } else if self.quota.max_storage_per_tenant == Some(HumanByteSize::b(0)) {
            Err(SnapshotConfigError::ZeroStorageQuota)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        schema::snapshot::{PreparationConfig, SnapshotConfig, SnapshotConfigError, SnapshotQuotaConfig},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn the_defaults_are_valid_and_set_no_quota() {
        let config = SnapshotConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.quota, SnapshotQuotaConfig::default());
        assert_eq!(config.quota.max_per_tenant, None);
    }

    #[test]
    fn a_default_lifetime_above_the_maximum_is_refused_and_an_equal_one_accepted() {
        let mut config = SnapshotConfig {
            default_lifetime: HumanDuration::from_secs(61),
            max_lifetime: HumanDuration::from_secs(60),
            ..SnapshotConfig::default()
        };
        assert_eq!(config.validate(), Err(SnapshotConfigError::DefaultLifetimeExceedsMax));
        config.default_lifetime = HumanDuration::from_secs(60);
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn zero_preparation_durations_are_refused() {
        let zero = HumanDuration::from_secs(0);
        for (preparation, error) in [
            (
                PreparationConfig {
                    scoped_operation_timeout: zero,
                    ..PreparationConfig::default()
                },
                SnapshotConfigError::ZeroScopedOperationTimeout,
            ),
            (
                PreparationConfig {
                    lease_timeout: zero,
                    ..PreparationConfig::default()
                },
                SnapshotConfigError::ZeroLeaseTimeout,
            ),
            (
                PreparationConfig {
                    recovery_interval: zero,
                    ..PreparationConfig::default()
                },
                SnapshotConfigError::ZeroRecoveryInterval,
            ),
        ] {
            let config = SnapshotConfig {
                preparation,
                ..SnapshotConfig::default()
            };
            assert_eq!(config.validate(), Err(error));
        }
    }

    #[test]
    fn a_zero_storage_quota_is_refused() {
        let config = SnapshotConfig {
            quota: SnapshotQuotaConfig {
                max_storage_per_tenant: Some(HumanByteSize::b(0)),
                ..SnapshotQuotaConfig::default()
            },
            ..SnapshotConfig::default()
        };
        assert_eq!(config.validate(), Err(SnapshotConfigError::ZeroStorageQuota));
    }
}
