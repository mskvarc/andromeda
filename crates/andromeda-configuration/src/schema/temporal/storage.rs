//! The `[temporal.storage]` tables: how history is partitioned, converted to
//! the columnstore, and aged out.

use crate::{
    human_duration::HumanDuration,
    schema::{feature_mode::FeatureMode, temporal::error::TemporalConfigError},
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use toml_comment::TomlComment;

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
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ColumnstoreConfig {
    /// The age past which a history partition is converted, as a whole number
    /// of days in human-readable form (`"30days"`). Not below the chunk
    /// interval, or a partition still receiving rows would be converted.
    /// Ignored on an edition without a columnstore.
    #[toml_comment(inline)]
    pub convert_after: HumanDuration,
    /// How often partitions past `convert_after` are converted, in
    /// human-readable form (`"1h"`). The first sweep runs when a tenant opens.
    #[toml_comment(inline)]
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
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetentionConfig {
    /// Whether history older than `delete_after` is deleted: `enabled` or
    /// `disabled`. Off by default: a broker never deletes history it was not
    /// told to.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
    /// The age past which history is deleted, as a whole number of days in
    /// human-readable form (`"365days"`). Required when `mode` is `enabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_after: Option<HumanDuration>,
    /// How often history past `delete_after` is deleted, in human-readable
    /// form (`"1h"`).
    #[toml_comment(inline)]
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
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemporalStorageConfig {
    /// The span one write-side history partition covers, as a whole number of
    /// days in human-readable form (`"7days"`). Left out, the schema's own
    /// default stands. Reconciled against the stored partitioning when a
    /// tenant opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_interval: Option<HumanDuration>,
    /// When a history partition is converted to the columnstore.
    pub columnstore: ColumnstoreConfig,
    /// Whether, and at what age, history is deleted.
    pub retention: RetentionConfig,
}

/// Whether a duration is a positive whole number of days.
const fn is_positive_whole_days(duration: HumanDuration) -> bool {
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
    /// Validate the day grains, the columnstore lag and the cadences.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalConfigError`] naming the first value refused.
    pub const fn validate(&self) -> Result<(), TemporalConfigError> {
        if let Some(chunk_interval) = self.chunk_interval
            && !is_positive_whole_days(chunk_interval)
        {
            return Err(TemporalConfigError::ChunkIntervalNotWholeDays);
        }
        if !is_positive_whole_days(self.columnstore.convert_after) {
            return Err(TemporalConfigError::ConvertAfterNotWholeDays);
        }
        let chunk_interval = match self.chunk_interval {
            Some(chunk_interval) => chunk_interval,
            None => DEFAULT_CHUNK_INTERVAL,
        };
        if self.columnstore.convert_after.duration().as_secs() < chunk_interval.duration().as_secs() {
            return Err(TemporalConfigError::ConvertAfterBelowChunkInterval);
        }
        if self.columnstore.sweep_interval.is_zero() {
            return Err(TemporalConfigError::ZeroColumnstoreSweepInterval);
        }
        match (self.retention.mode, self.retention.delete_after) {
            (FeatureMode::Enabled, None) => return Err(TemporalConfigError::RetentionWithoutDeleteAfter),
            (FeatureMode::Enabled | FeatureMode::Disabled, Some(delete_after)) if !is_positive_whole_days(delete_after) => {
                return Err(TemporalConfigError::DeleteAfterNotWholeDays);
            }
            (FeatureMode::Enabled | FeatureMode::Disabled, Some(_)) | (FeatureMode::Disabled, None) => {}
        }
        if self.retention.sweep_interval.is_zero() {
            return Err(TemporalConfigError::ZeroRetentionSweepInterval);
        }
        Ok(())
    }

    /// The whole days the chunk interval override spans, absent when no
    /// override is set.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalConfigError::ChunkIntervalNotWholeDays`] for a value
    /// that is not a positive whole number of days, which
    /// [`TemporalStorageConfig::validate`] refuses first.
    pub fn chunk_interval_days(&self) -> Result<Option<NonZeroU32>, TemporalConfigError> {
        self.chunk_interval
            .map(|chunk_interval| whole_days(chunk_interval).ok_or(TemporalConfigError::ChunkIntervalNotWholeDays))
            .transpose()
    }

    /// The whole days after which a history partition is converted to the
    /// columnstore.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalConfigError::ConvertAfterNotWholeDays`] for a value
    /// that is not a positive whole number of days, which
    /// [`TemporalStorageConfig::validate`] refuses first.
    pub fn convert_after_days(&self) -> Result<NonZeroU32, TemporalConfigError> {
        whole_days(self.columnstore.convert_after).ok_or(TemporalConfigError::ConvertAfterNotWholeDays)
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
            temporal::{
                error::TemporalConfigError,
                storage::{ColumnstoreConfig, RetentionConfig, TemporalStorageConfig},
            },
        },
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn retention_is_off_and_the_defaults_valid() {
        let config = TemporalStorageConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.retention_age(), None);
        assert_eq!(config.columnstore.sweep_interval.duration(), Duration::from_secs(3600));
    }

    #[test]
    fn enabled_retention_without_an_age_is_refused_and_with_one_reads_it() {
        let mut config = TemporalStorageConfig {
            retention: RetentionConfig {
                mode: FeatureMode::Enabled,
                ..TemporalStorageConfig::default().retention
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::RetentionWithoutDeleteAfter));
        config.retention.delete_after = Some(HumanDuration::from_days(365));
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.retention_age(), Some(HumanDuration::from_days(365)));
    }

    #[test]
    fn a_sub_day_or_fractional_day_value_is_refused() {
        let fractional_chunk = TemporalStorageConfig {
            chunk_interval: Some(HumanDuration::new(Duration::from_hours(36))),
            ..TemporalStorageConfig::default()
        };
        assert_eq!(fractional_chunk.validate(), Err(TemporalConfigError::ChunkIntervalNotWholeDays));
        let sub_day_age = TemporalStorageConfig {
            retention: RetentionConfig {
                mode: FeatureMode::Enabled,
                delete_after: Some(HumanDuration::new(Duration::from_hours(12))),
                ..TemporalStorageConfig::default().retention
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(sub_day_age.validate(), Err(TemporalConfigError::DeleteAfterNotWholeDays));
        let zero_lag = TemporalStorageConfig {
            columnstore: ColumnstoreConfig {
                convert_after: HumanDuration::from_days(0),
                ..ColumnstoreConfig::default()
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(zero_lag.validate(), Err(TemporalConfigError::ConvertAfterNotWholeDays));
    }

    #[test]
    fn a_columnstore_lag_shorter_than_the_chunk_interval_is_refused_against_either_default() {
        let short_lag = TemporalStorageConfig {
            columnstore: ColumnstoreConfig {
                convert_after: HumanDuration::from_days(3),
                ..ColumnstoreConfig::default()
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(short_lag.validate(), Err(TemporalConfigError::ConvertAfterBelowChunkInterval));
        let long_chunk = TemporalStorageConfig {
            chunk_interval: Some(HumanDuration::from_days(60)),
            ..TemporalStorageConfig::default()
        };
        assert_eq!(long_chunk.validate(), Err(TemporalConfigError::ConvertAfterBelowChunkInterval));
        let both = TemporalStorageConfig {
            chunk_interval: Some(HumanDuration::from_days(1)),
            ..short_lag
        };
        assert_eq!(both.validate(), Ok(()));
    }

    #[test]
    fn a_zero_sweep_cadence_is_refused() {
        let columnstore = TemporalStorageConfig {
            columnstore: ColumnstoreConfig {
                sweep_interval: HumanDuration::from_secs(0),
                ..ColumnstoreConfig::default()
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(columnstore.validate(), Err(TemporalConfigError::ZeroColumnstoreSweepInterval));
        let retention = TemporalStorageConfig {
            retention: RetentionConfig {
                sweep_interval: HumanDuration::from_secs(0),
                ..TemporalStorageConfig::default().retention
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(retention.validate(), Err(TemporalConfigError::ZeroRetentionSweepInterval));
    }
}
