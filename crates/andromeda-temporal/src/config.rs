//! Temporal policy configuration.

use crate::storage_policy::TemporalStoragePolicy;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// The chunk interval the tenant schema declares when no override is set.
///
/// Mirrors the `create_hypertable` interval in the attribute-history and
/// scope-history migrations; validation compares an override of one value
/// against the default of the other.
pub const DEFAULT_CHUNK_INTERVAL_DAYS: u32 = 7;

/// The columnstore lag the tenant schema declares when no override is set.
///
/// Mirrors the `add_columnstore_policy` lag in the attribute-history and
/// scope-history migrations.
pub const DEFAULT_COLUMNSTORE_AFTER_DAYS: u32 = 30;

/// Invalid temporal policy configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TemporalConfigError {
    /// The default `lastN` limit was configured as zero.
    #[error("temporal.default_last_n must be at least 1")]
    ZeroDefaultLastN,
    /// Expiry reconciliation cadence was configured as zero.
    #[error("temporal.expiration_check_interval_secs must be at least 1")]
    ZeroExpirationCheckInterval,
    /// Intent fallback cadence was configured as zero.
    #[error("temporal.intent_fallback_secs must be at least 1")]
    ZeroIntentFallback,
    /// Intent batch size was configured as zero.
    #[error("temporal.intent_batch_size must be at least 1")]
    ZeroIntentBatchSize,
    /// Intent lease duration was configured as zero.
    #[error("temporal.intent_lease_secs must be at least 1")]
    ZeroIntentLease,
    /// Intent retry delay was configured as zero.
    #[error("temporal.intent_retry_secs must be at least 1")]
    ZeroIntentRetry,
    /// Intent retry ceiling was configured below the base delay.
    #[error("temporal.intent_retry_max_secs must be at least temporal.intent_retry_secs")]
    IntentRetryCeilingBelowBase,
    /// The stuck-intent warning threshold was configured as zero.
    #[error("temporal.intent_stuck_after_attempts must be at least 1")]
    ZeroIntentStuckThreshold,
    /// The chunk interval override was configured as zero.
    #[error("temporal.chunk_interval_days must be at least 1")]
    ZeroChunkInterval,
    /// The columnstore lag override was configured as zero.
    #[error("temporal.columnstore_after_days must be at least 1")]
    ZeroColumnstoreLag,
    /// The columnstore lag would convert a chunk that is still being written.
    #[error("temporal.columnstore_after_days must be at least temporal.chunk_interval_days")]
    ColumnstoreLagBelowChunkInterval,
    /// A retention horizon was named but spans no time.
    #[error("temporal.retention_horizon_days must be at least 1 when retention is enabled")]
    ZeroRetentionHorizon,
    /// Retention sweep cadence was configured as zero.
    #[error("temporal.retention_sweep_interval_secs must be at least 1")]
    ZeroRetentionSweepInterval,
}

/// How the PostgreSQL anchor lands temporal append consequences.
#[derive(Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum TemporalAppendMode {
    /// Persist temporal history inside the anchor transaction.
    #[default]
    Native,
    /// Persist a typed intent in the anchor and deliver it through the selected sink.
    Deferred,
}

/// Defaults and reconciliation cadence for temporal history.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct TemporalConfig {
    /// Native or deferred append topology selected for this broker profile.
    #[toml_comment(inline)]
    pub append_mode: TemporalAppendMode,
    /// System default `lastN` cap applied when a temporal query omits `lastN`.
    ///
    /// Clause 5.2.21 leaves the default implementation-defined. It is kept at 19
    /// so it sits just under the ETSI conformance suite's precondition of
    /// pagination at 20 instances per attribute: the suite expects a default
    /// query to page rather than return every instance. Raising it is sound for
    /// production, but must first be reconciled with that suite precondition and
    /// with the `SYSTEM_LAST_N_CAP` regression tests, which assert the 206 and
    /// `Content-Range` that only appear when a result is actually cut by this cap.
    pub default_last_n: u32,
    /// Cadence, in seconds, of every `expiresAt` sweep the broker schedules
    /// per tenant: entities (clause 5.2.4), subscriptions (clause 5.8.1.4),
    /// context source registrations (clause 5.9.2.4), snapshots and entity
    /// maps. Each sweep deletes what has expired at the moment it runs, so a
    /// resource may outlive its `expiresAt` in storage by up to one cadence;
    /// the read paths hide an expired registration in the meantime, so the
    /// API never serves one. The conformance stacks run this at 5 s.
    pub expiration_check_interval_secs: u64,
    /// Fallback poll cadence for committed temporal intents.
    pub intent_fallback_secs: u64,
    /// Maximum intents claimed by one bounded drain pass.
    pub intent_batch_size: u32,
    /// Lease duration for one claimed intent batch.
    pub intent_lease_secs: u64,
    /// Base retry delay for a sink rejection.
    pub intent_retry_secs: u64,
    /// Ceiling the exponential retry delay is clamped to.
    pub intent_retry_max_secs: u64,
    /// Attempt count from which an intent that keeps failing is reported as stuck.
    ///
    /// There is no dead-letter for a temporal append intent: the intent is the
    /// durable record of history the broker accepted, so it stays claimable
    /// forever and is only reported, never discarded.
    pub intent_stuck_after_attempts: u32,
    /// Override of the span one write-side history partition covers, in days.
    ///
    /// Absent, the schema's own default stands. Reconciled against the stored
    /// hypertable dimension when a tenant is opened.
    pub chunk_interval_days: Option<u32>,
    /// Override of the age past which a history partition is converted to the
    /// columnstore, in days.
    ///
    /// Absent, the schema's own default stands. Must not fall below the chunk
    /// interval, or a partition still receiving rows would be converted.
    pub columnstore_after_days: Option<u32>,
    /// Operator retention horizon in days, absent while retention is off.
    ///
    /// Retention ages temporal storage out and is not an NGSI-LD operation: the
    /// specification defines deletion of a Temporal Evolution (clause 5.6.16)
    /// and says nothing about storage an operator chooses not to keep. It is
    /// therefore absent unless an operator names a horizon.
    pub retention_horizon_days: Option<u64>,
    /// Cadence of the operator retention sweep.
    pub retention_sweep_interval_secs: u64,
}

impl Default for TemporalConfig {
    fn default() -> TemporalConfig {
        TemporalConfig {
            append_mode: TemporalAppendMode::Native,
            default_last_n: 19,
            expiration_check_interval_secs: 30,
            intent_fallback_secs: 1,
            intent_batch_size: 64,
            intent_lease_secs: 30,
            intent_retry_secs: 2,
            intent_retry_max_secs: 300,
            intent_stuck_after_attempts: 10,
            chunk_interval_days: None,
            columnstore_after_days: None,
            retention_horizon_days: None,
            retention_sweep_interval_secs: 3600,
        }
    }
}

impl TemporalConfig {
    /// Validate temporal caps, drain limits, and cadences.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalConfigError`] variant naming the first setting
    /// that spans no time or no work: a zero `lastN` cap, expiry cadence,
    /// intent fallback cadence, batch size, lease, retry delay, stuck-attempt
    /// threshold, chunk interval, columnstore lag, retention horizon or
    /// retention sweep cadence; a retry ceiling below the retry delay; or a
    /// columnstore lag below the chunk interval, which would convert a
    /// partition that is still being written.
    pub const fn validate(&self) -> Result<(), TemporalConfigError> {
        if self.default_last_n == 0 {
            return Err(TemporalConfigError::ZeroDefaultLastN);
        }
        if self.expiration_check_interval_secs == 0 {
            return Err(TemporalConfigError::ZeroExpirationCheckInterval);
        }
        if self.intent_fallback_secs == 0 {
            return Err(TemporalConfigError::ZeroIntentFallback);
        }
        if self.intent_batch_size == 0 {
            return Err(TemporalConfigError::ZeroIntentBatchSize);
        }
        if self.intent_lease_secs == 0 {
            return Err(TemporalConfigError::ZeroIntentLease);
        }
        if self.intent_retry_secs == 0 {
            return Err(TemporalConfigError::ZeroIntentRetry);
        }
        if self.intent_retry_max_secs < self.intent_retry_secs {
            return Err(TemporalConfigError::IntentRetryCeilingBelowBase);
        }
        if self.intent_stuck_after_attempts == 0 {
            return Err(TemporalConfigError::ZeroIntentStuckThreshold);
        }
        if let Some(0) = self.chunk_interval_days {
            return Err(TemporalConfigError::ZeroChunkInterval);
        }
        if let Some(0) = self.columnstore_after_days {
            return Err(TemporalConfigError::ZeroColumnstoreLag);
        }
        let chunk_interval_days = match self.chunk_interval_days {
            Some(days) => days,
            None => DEFAULT_CHUNK_INTERVAL_DAYS,
        };
        let columnstore_after_days = match self.columnstore_after_days {
            Some(days) => days,
            None => DEFAULT_COLUMNSTORE_AFTER_DAYS,
        };
        if columnstore_after_days < chunk_interval_days {
            return Err(TemporalConfigError::ColumnstoreLagBelowChunkInterval);
        }
        if let Some(0) = self.retention_horizon_days {
            return Err(TemporalConfigError::ZeroRetentionHorizon);
        }
        if self.retention_sweep_interval_secs == 0 {
            return Err(TemporalConfigError::ZeroRetentionSweepInterval);
        }
        Ok(())
    }

    /// The storage policy the overrides describe, for the backend to reconcile.
    ///
    /// Only meaningful on a validated configuration: a zero override is
    /// refused by [`TemporalConfig::validate`] and treated as absent here.
    #[must_use]
    pub const fn storage_policy(&self) -> TemporalStoragePolicy {
        TemporalStoragePolicy {
            chunk_interval_days: match self.chunk_interval_days {
                Some(days) => NonZeroU32::new(days),
                None => None,
            },
            columnstore_after_days: match self.columnstore_after_days {
                Some(days) => NonZeroU32::new(days),
                None => None,
            },
        }
    }
}

#[cfg(test)]
mod config_tests {
    use super::{TemporalAppendMode, TemporalConfig, TemporalConfigError};
    use crate::storage_policy::TemporalStoragePolicy;
    use std::num::NonZeroU32;

    fn valid() -> TemporalConfig {
        TemporalConfig {
            append_mode: TemporalAppendMode::Native,
            default_last_n: 19,
            expiration_check_interval_secs: 30,
            intent_fallback_secs: 1,
            intent_batch_size: 64,
            intent_lease_secs: 30,
            intent_retry_secs: 2,
            intent_retry_max_secs: 300,
            intent_stuck_after_attempts: 10,
            chunk_interval_days: None,
            columnstore_after_days: None,
            retention_horizon_days: None,
            retention_sweep_interval_secs: 3600,
        }
    }

    #[test]
    fn a_retry_ceiling_below_the_base_delay_is_rejected() {
        let config = TemporalConfig {
            intent_retry_secs: 10,
            intent_retry_max_secs: 5,
            ..valid()
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::IntentRetryCeilingBelowBase));
    }

    #[test]
    fn a_zero_stuck_threshold_is_rejected() {
        let config = TemporalConfig {
            intent_stuck_after_attempts: 0,
            ..valid()
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::ZeroIntentStuckThreshold));
    }

    #[test]
    fn a_zero_storage_override_is_rejected() {
        let chunk = TemporalConfig {
            chunk_interval_days: Some(0),
            ..valid()
        };
        assert_eq!(chunk.validate(), Err(TemporalConfigError::ZeroChunkInterval));
        let lag = TemporalConfig {
            columnstore_after_days: Some(0),
            ..valid()
        };
        assert_eq!(lag.validate(), Err(TemporalConfigError::ZeroColumnstoreLag));
    }

    #[test]
    fn a_columnstore_lag_shorter_than_the_chunk_interval_is_rejected_against_either_default() {
        let short_lag = TemporalConfig {
            columnstore_after_days: Some(3),
            ..valid()
        };
        assert_eq!(short_lag.validate(), Err(TemporalConfigError::ColumnstoreLagBelowChunkInterval));
        let long_chunk = TemporalConfig {
            chunk_interval_days: Some(60),
            ..valid()
        };
        assert_eq!(long_chunk.validate(), Err(TemporalConfigError::ColumnstoreLagBelowChunkInterval));
        let both = TemporalConfig {
            chunk_interval_days: Some(1),
            columnstore_after_days: Some(3),
            ..valid()
        };
        assert_eq!(both.validate(), Ok(()));
    }

    #[test]
    fn the_storage_policy_carries_exactly_the_overrides_set() {
        assert_eq!(
            valid().storage_policy(),
            TemporalStoragePolicy {
                chunk_interval_days: None,
                columnstore_after_days: None,
            }
        );
        let overridden = TemporalConfig {
            chunk_interval_days: Some(1),
            columnstore_after_days: Some(3),
            ..valid()
        };
        assert_eq!(
            overridden.storage_policy(),
            TemporalStoragePolicy {
                chunk_interval_days: NonZeroU32::new(1),
                columnstore_after_days: NonZeroU32::new(3),
            }
        );
    }

    #[test]
    fn zero_intent_batch_size_is_rejected() {
        let config = TemporalConfig {
            intent_batch_size: 0,
            ..valid()
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::ZeroIntentBatchSize));
    }

    #[test]
    fn retention_is_off_unless_an_operator_names_a_horizon() {
        let config = valid();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.retention_horizon_days, None);
    }

    #[test]
    fn a_retention_horizon_spanning_no_time_is_rejected() {
        let config = TemporalConfig {
            retention_horizon_days: Some(0),
            ..valid()
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::ZeroRetentionHorizon));
    }
}
