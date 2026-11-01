//! The `[temporal.storage]` tables: how history is partitioned, converted to
//! the columnstore, and aged out.

use crate::{human_duration::HumanDuration, schema::feature_mode::FeatureMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use thiserror::Error;

/// The chunk interval the tenant schema declares when no override is set.
///
/// Mirrors the `create_hypertable` interval in the attribute-history and
/// scope-history migrations; validation compares an override of one value
/// against the default of the other.
pub const DEFAULT_CHUNK_INTERVAL: HumanDuration = HumanDuration::from_days(7);

/// The columnstore lag the broker converts under when no other is configured:
/// the thirty-day horizon within which history is expected to be mutated in
/// place.
pub const DEFAULT_CONVERT_AFTER: HumanDuration = HumanDuration::from_days(30);

/// The number of seconds in one day, the grain the partition day fields are
/// constrained to.
const SECONDS_PER_DAY: u64 = 86_400;

/// When a history partition is converted to the columnstore.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "When history partitions are converted to the columnstore.")]
pub struct ColumnstoreConfig {
    /// The age past which a history partition is converted, as a whole number
    /// of days in human-readable form (`"30days"`). Not below the chunk
    /// interval, or a partition still receiving rows would be converted.
    /// Ignored on an edition without a columnstore.
    #[schemars(
        description = "The age past which a history partition is converted to the columnstore. Only the TimescaleDB TSL edition has a columnstore; on the Apache-2.0 edition the key has no effect."
    )]
    pub convert_after: HumanDuration,
    /// How often partitions past `convert_after` are converted, in
    /// human-readable form (`"1h"`). The first sweep runs when a tenant opens.
    #[schemars(
        description = "How often partitions past `temporal.storage.columnstore.convert_after` are converted. The first sweep runs when a tenant opens."
    )]
    pub sweep_interval: HumanDuration,
}

impl Default for ColumnstoreConfig {
    fn default() -> ColumnstoreConfig {
        ColumnstoreConfig {
            convert_after: DEFAULT_CONVERT_AFTER,
            sweep_interval: HumanDuration::from_secs(3600),
        }
    }
}

/// Whether, and at what age, the operator's history is deleted.
///
/// Retention ages temporal storage out and is not an NGSI-LD operation: the
/// specification defines deletion of a Temporal Evolution (clause 5.6.16)
/// and says nothing about storage an operator chooses not to keep.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(
    description = "Whether, and at what age, history is deleted. Retention is a storage decision, not an NGSI-LD deletion: history past the age leaves the database without a deletion record and without a notification."
)]
pub struct RetentionConfig {
    /// Whether history older than `delete_after` is deleted: `enabled` or
    /// `disabled`. Off by default: a broker never deletes history it was not
    /// told to.
    #[schemars(
        description = "Whether history older than `temporal.storage.retention.delete_after` is deleted. A broker never deletes history it was not told to."
    )]
    pub mode: FeatureMode,
    /// The age past which history is deleted, as a whole number of days in
    /// human-readable form (`"365days"`). Required when `mode` is `enabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "How much history to keep. History partitions recorded entirely before this age are dropped, together with the Scope history no remaining instance needs. Left out, no age is set and no history is deleted."
    )]
    pub delete_after: Option<HumanDuration>,
    /// How often history past `delete_after` is deleted, in human-readable
    /// form (`"1h"`). The first sweep runs when a tenant opens.
    #[schemars(
        description = "How often history past `temporal.storage.retention.delete_after` is deleted while retention is enabled. The first sweep runs when a tenant opens."
    )]
    pub sweep_interval: HumanDuration,
}

impl Default for RetentionConfig {
    fn default() -> RetentionConfig {
        RetentionConfig {
            mode: FeatureMode::Disabled,
            delete_after: None,
            sweep_interval: HumanDuration::from_secs(3600),
        }
    }
}

/// How history is partitioned, converted to the columnstore, and aged out.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "How history is partitioned, converted to the columnstore and aged out.")]
pub struct TemporalStorageConfig {
    /// The span one write-side history partition covers, as a whole number of
    /// days in human-readable form (`"7days"`). Left out, the schema's own
    /// default stands. Reconciled against the stored partitioning when a
    /// tenant opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "The span of time one history partition covers. Left out, the broker leaves each history table's partitioning as it stands, which is 7 days on a table it created. A change is reconciled against the stored partitioning when the broker next opens the tenant; shorten it if the largest partition still in the rowstore keeps passing 4 GB."
    )]
    pub chunk_interval: Option<HumanDuration>,
    /// When a history partition is converted to the columnstore.
    #[schemars(description = "When history partitions are converted to the columnstore.")]
    pub columnstore: ColumnstoreConfig,
    /// Whether, and at what age, history is deleted.
    #[schemars(description = "Whether, and at what age, history is deleted.")]
    pub retention: RetentionConfig,
}

/// A day-grained `[temporal.storage]` duration that is not a positive whole
/// number of days, which validation refuses before the duration is read.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum StorageDaysError {
    /// `chunk_interval` is not a positive whole number of days.
    #[error("temporal.storage.chunk_interval must be a positive whole number of days")]
    ChunkInterval,
    /// `columnstore.convert_after` is not a positive whole number of days.
    #[error("temporal.storage.columnstore.convert_after must be a positive whole number of days")]
    ConvertAfter,
    /// `retention.delete_after` is not a positive whole number of days.
    #[error("temporal.storage.retention.delete_after must be a positive whole number of days")]
    DeleteAfter,
}

/// Whether a duration is a positive whole number of days.
pub(crate) const fn is_positive_whole_days(duration: HumanDuration) -> bool {
    let seconds = duration.duration().as_secs();
    seconds != 0 && seconds.is_multiple_of(SECONDS_PER_DAY)
}

/// The whole days a day-grained duration spans, or [`None`] for one that is
/// not a positive whole number of days or overflows [`u32`].
fn whole_days(duration: HumanDuration) -> Option<NonZeroU32> {
    let days = duration.duration().as_secs() / SECONDS_PER_DAY;
    u32::try_from(days).ok().and_then(NonZeroU32::new)
}

impl TemporalStorageConfig {
    /// The whole days the chunk interval override spans, absent when no
    /// override is set.
    ///
    /// # Errors
    ///
    /// Returns [`StorageDaysError::ChunkInterval`] for a value that is not a
    /// positive whole number of days, which validation refuses first.
    pub fn chunk_interval_days(&self) -> Result<Option<NonZeroU32>, StorageDaysError> {
        self.chunk_interval
            .map(|chunk_interval| whole_days(chunk_interval).ok_or(StorageDaysError::ChunkInterval))
            .transpose()
    }

    /// The whole days after which a history partition is converted to the
    /// columnstore.
    ///
    /// # Errors
    ///
    /// Returns [`StorageDaysError::ConvertAfter`] for a value that is not a
    /// positive whole number of days, which validation refuses first.
    pub fn convert_after_days(&self) -> Result<NonZeroU32, StorageDaysError> {
        whole_days(self.columnstore.convert_after).ok_or(StorageDaysError::ConvertAfter)
    }

    /// The whole days past which history is deleted, absent while retention
    /// is off.
    ///
    /// # Errors
    ///
    /// Returns [`StorageDaysError::DeleteAfter`] for a value that is not a
    /// positive whole number of days, which validation refuses first.
    pub fn delete_after_days(&self) -> Result<Option<NonZeroU32>, StorageDaysError> {
        self.retention_age()
            .map(|delete_after| whole_days(delete_after).ok_or(StorageDaysError::DeleteAfter))
            .transpose()
    }

    /// The age past which history is deleted, absent while retention is off.
    #[must_use]
    pub const fn retention_age(&self) -> Option<HumanDuration> {
        match self.retention.mode {
            FeatureMode::Enabled => self.retention.delete_after,
            FeatureMode::Disabled => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::{
            feature_mode::FeatureMode,
            temporal::storage::{RetentionConfig, StorageDaysError, TemporalStorageConfig},
        },
    };
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    #[test]
    fn retention_is_off_by_default() {
        let config = TemporalStorageConfig::default();
        assert_eq!(config.retention_age(), None);
        assert_eq!(config.columnstore.sweep_interval.duration(), Duration::from_secs(3600));
    }

    #[test]
    fn enabled_retention_reads_its_age() {
        let config = TemporalStorageConfig {
            retention: RetentionConfig {
                mode: FeatureMode::Enabled,
                delete_after: Some(HumanDuration::from_days(365)),
                ..TemporalStorageConfig::default().retention
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(config.retention_age(), Some(HumanDuration::from_days(365)));
    }

    #[test]
    fn the_day_grains_read_as_whole_days() {
        let mut config = TemporalStorageConfig::default();
        assert_eq!(config.chunk_interval_days(), Ok(None));
        assert_eq!(config.convert_after_days(), Ok(NonZeroU32::MIN.saturating_add(29)));
        config.chunk_interval = Some(HumanDuration::from_days(0));
        config.columnstore.convert_after = HumanDuration::from_days(0);
        assert_eq!(config.chunk_interval_days(), Err(StorageDaysError::ChunkInterval));
        assert_eq!(config.convert_after_days(), Err(StorageDaysError::ConvertAfter));
    }

    #[test]
    fn the_retention_age_reads_as_whole_days_only_while_enabled() {
        let mut config = TemporalStorageConfig::default();
        config.retention.delete_after = Some(HumanDuration::from_days(30));
        assert_eq!(config.delete_after_days(), Ok(None));
        config.retention.mode = FeatureMode::Enabled;
        assert_eq!(config.delete_after_days(), Ok(NonZeroU32::new(30)));
        config.retention.delete_after = Some(HumanDuration::from_secs(3600));
        assert_eq!(config.delete_after_days(), Err(StorageDaysError::DeleteAfter));
    }
}
