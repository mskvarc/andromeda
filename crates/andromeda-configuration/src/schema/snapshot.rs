//! The `[snapshot]` tables: how long a Snapshot lives, how many a tenant may
//! keep, and how one is prepared.

use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use toml_comment::TomlComment;

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

#[cfg(test)]
mod tests {
    use crate::schema::snapshot::{SnapshotConfig, SnapshotQuotaConfig};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_defaults_set_no_quota() {
        let config = SnapshotConfig::default();
        assert_eq!(config.quota, SnapshotQuotaConfig::default());
        assert_eq!(config.quota.max_per_tenant, None);
    }
}
