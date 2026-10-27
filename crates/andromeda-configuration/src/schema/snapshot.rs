//! The `[snapshot]` tables: how long a Snapshot lives, how many a tenant may
//! keep, and how one is prepared.

use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

/// How many Snapshots one tenant may keep. Each key left out is no limit.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[schemars(
    description = "How many Snapshots, and how much storage, one tenant may keep. Over a quota, Snapshots are deleted until the tenant is within it again: the lowest `snapshotPriority` first, then the one expiring soonest, then the one used least recently."
)]
#[serde(deny_unknown_fields)]
pub struct SnapshotQuotaConfig {
    /// The most Snapshots one tenant keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "The most Snapshots one tenant keeps. Left out, the count is not limited.")]
    pub max_per_tenant: Option<NonZeroUsize>,
    /// The most storage one tenant's Snapshots hold, in human-readable form
    /// (`"10 GiB"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "The most storage one tenant's Snapshots take. Left out, their storage is not limited.")]
    pub max_storage_per_tenant: Option<HumanByteSize>,
}

/// How a Snapshot is prepared.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "How a Snapshot's content is captured or cloned after it is created.")]
#[serde(deny_unknown_fields)]
pub struct PreparationConfig {
    /// The results read in each capture page.
    #[schemars(description = "The results read per page while a Snapshot captures its content.")]
    pub page_size: NonZeroUsize,
    /// The most Snapshots prepared at once.
    #[schemars(description = "The most Snapshots prepared at once.")]
    pub max_concurrent: NonZeroUsize,
    /// How long a claim keeps a preparation from every other worker, in
    /// human-readable form (`"30s"`).
    #[schemars(description = "How long a claim keeps a preparation from every other worker before another may take it over.")]
    pub lease_timeout: HumanDuration,
    /// How often preparations a crash or a missed run left unfinished are
    /// resumed, in human-readable form (`"30s"`). Creation prepares its
    /// Snapshot at once, so this sweep only recovers.
    #[schemars(
        description = "How often preparations a crash or a missed run left unfinished are resumed. Creating a Snapshot starts preparing it at once, so this sweep only recovers."
    )]
    pub recovery_interval: HumanDuration,
    /// How long a scoped operation waits for capture or clone promotion, in
    /// human-readable form (`"30s"`).
    #[schemars(description = "How long a request scoped to a Snapshot waits for its capture or clone to finish.")]
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
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "How long a Snapshot lives, how many a tenant may keep, and how one is prepared.")]
#[serde(deny_unknown_fields)]
pub struct SnapshotConfig {
    /// The lifetime of a Snapshot created without `snapshotLifetime`, in
    /// human-readable form (`"1h"`).
    #[schemars(description = "The lifetime of a Snapshot created without `snapshotLifetime`.")]
    pub default_lifetime: HumanDuration,
    /// The longest a Snapshot lives; a longer requested `snapshotLifetime`
    /// is cut down to it.
    #[schemars(description = "The longest a Snapshot lives. A longer requested `snapshotLifetime` is cut down to this.")]
    pub max_lifetime: HumanDuration,
    /// How many Snapshots one tenant may keep.
    #[schemars(description = "How many Snapshots, and how much storage, one tenant may keep.")]
    pub quota: SnapshotQuotaConfig,
    /// How a Snapshot is prepared.
    #[schemars(description = "How a Snapshot is prepared.")]
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
