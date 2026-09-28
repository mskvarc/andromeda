//! Temporal policy configuration.

use crate::{instance_limits::TemporalInstanceLimits, storage_policy::TemporalStoragePolicy};
use andromeda_configuration_scalars::human_duration::HumanDuration;
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
pub const DEFAULT_CHUNK_INTERVAL: HumanDuration = HumanDuration::from_days(7);

/// The columnstore lag the broker enables the columnstore under when no other
/// is configured: the thirty-day horizon within which history is expected to be
/// mutated in place.
pub const DEFAULT_COLUMNSTORE_AFTER: HumanDuration = HumanDuration::from_days(30);

/// The largest `temporal.max_last_n` a deployment may configure.
///
/// Every instance of a temporal page is rendered and compacted in memory before
/// the response is written, so the ceiling bounds the work one request can ask
/// of the broker regardless of how an operator tunes the maximum below it.
pub const MAX_LAST_N_CEILING: u32 = 5000;

/// The number of seconds in one day, the grain the history-partition day fields
/// are constrained to.
const SECONDS_PER_DAY: u64 = 86_400;

/// Invalid temporal policy configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TemporalConfigError {
    /// The default `lastN` limit was configured as zero.
    #[error("temporal.default_last_n must be at least 1")]
    ZeroDefaultLastN,
    /// The `lastN` maximum was configured below the default page.
    ///
    /// A read without `lastN` is paged at the default, and that page has to be
    /// one a client could also ask for explicitly.
    #[error("temporal.max_last_n must be at least temporal.default_last_n")]
    MaxLastNBelowDefault,
    /// The `lastN` maximum was configured above [`MAX_LAST_N_CEILING`].
    #[error("temporal.max_last_n must not exceed {MAX_LAST_N_CEILING}")]
    MaxLastNAboveCeiling,
    /// Expiry reconciliation cadence was configured as zero.
    #[error("temporal.expiration_check_interval must be greater than zero")]
    ZeroExpirationCheckInterval,
    /// Intent fallback cadence was configured as zero.
    #[error("temporal.intent_fallback must be greater than zero")]
    ZeroIntentFallback,
    /// Intent fallback ceiling was configured below the fallback floor.
    #[error("temporal.intent_fallback_max must be at least temporal.intent_fallback")]
    IntentFallbackCeilingBelowFloor,
    /// Intent batch size was configured as zero.
    #[error("temporal.intent_batch_size must be at least 1")]
    ZeroIntentBatchSize,
    /// Intent lease duration was configured as zero.
    #[error("temporal.intent_lease must be greater than zero")]
    ZeroIntentLease,
    /// Intent retry delay was configured as zero.
    #[error("temporal.intent_retry must be greater than zero")]
    ZeroIntentRetry,
    /// Intent retry ceiling was configured below the base delay.
    #[error("temporal.intent_retry_max must be at least temporal.intent_retry")]
    IntentRetryCeilingBelowBase,
    /// The stuck-intent warning threshold was configured as zero.
    #[error("temporal.intent_stuck_after_attempts must be at least 1")]
    ZeroIntentStuckThreshold,
    /// The chunk interval override was not a positive whole number of days.
    ///
    /// The write-side history partitions are day-grained, so an override must
    /// resolve to whole days: a fractional or sub-day span such as `"36h"` has
    /// no partition boundary to land on.
    #[error("temporal.chunk_interval must be a positive whole number of days")]
    ChunkIntervalNotWholeDays,
    /// The columnstore lag override was not a positive whole number of days.
    #[error("temporal.columnstore_after must be a positive whole number of days")]
    ColumnstoreAfterNotWholeDays,
    /// The columnstore lag would convert a chunk that is still being written.
    #[error("temporal.columnstore_after must be at least temporal.chunk_interval")]
    ColumnstoreLagBelowChunkInterval,
    /// A retention horizon was named but was not a positive whole number of days.
    #[error("temporal.retention_horizon must be a positive whole number of days when retention is enabled")]
    RetentionHorizonNotWholeDays,
    /// Retention sweep cadence was configured as zero.
    #[error("temporal.retention_sweep_interval must be greater than zero")]
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
    /// Instances per attribute a temporal read without `lastN` returns before
    /// it is paged forwards (clause 6.3.10: "If no limits are specified, a
    /// default limit is enforced, depending on implementation specific
    /// configurations").
    ///
    /// Defaults to 100. The ETSI conformance suite has a precondition of
    /// pagination at 20 instances per attribute (a default query must page
    /// rather than return every instance), so a conformance run sets
    /// `ANDROMEDA__TEMPORAL__DEFAULT_LAST_N=19` to sit just under that threshold.
    pub default_last_n: u32,
    /// The largest `lastN` answered in full; a larger one is narrowed to this
    /// many instances per attribute and paged backwards with a 206 whose
    /// `Content-Range` still reports the requested `lastN` (clause 6.3.10).
    ///
    /// Defaults to 1000, and may be raised to at most 5000; it must not fall
    /// below `default_last_n`. The same number bounds the instances that share
    /// one timestamp at the edge of a page: a page never splits them, and a
    /// group larger than this is refused with `TooManyResults`. The ETSI
    /// conformance suite expects a `lastN` of 20 to be paged, so a conformance
    /// run sets `ANDROMEDA__TEMPORAL__MAX_LAST_N=19` beside the default.
    pub max_last_n: u32,
    /// Cadence of every `expiresAt` sweep the broker schedules per tenant:
    /// entities (clause 5.2.4), subscriptions (clause 5.8.1.4), context source
    /// registrations (clause 5.9.2.4), snapshots and entity maps, in
    /// human-readable form (`"30s"`). Each sweep deletes what has expired at the
    /// moment it runs, so a resource may outlive its `expiresAt` in storage by
    /// up to one cadence; the read paths hide an expired registration in the
    /// meantime, so the API never serves one. The conformance stacks run this at
    /// `"5s"`.
    #[toml_comment(inline)]
    pub expiration_check_interval: HumanDuration,
    /// Fallback poll cadence for committed temporal intents, in human-readable
    /// form (`"1s"`).
    ///
    /// The floor of the drain's idle backoff: after a commit's edge wake the
    /// drain runs promptly, and while idle it backs off from here toward
    /// `intent_fallback_max`.
    #[toml_comment(inline)]
    pub intent_fallback: HumanDuration,
    /// Ceiling the intent drain's idle backoff reaches while no commit wakes it,
    /// in human-readable form (`"30s"`).
    ///
    /// Also the cross-replica pickup bound: a committed intent another replica
    /// did not observe a wake for is claimed within this cadence.
    #[toml_comment(inline)]
    pub intent_fallback_max: HumanDuration,
    /// Maximum intents claimed by one bounded drain pass.
    pub intent_batch_size: u32,
    /// Lease duration for one claimed intent batch, in human-readable form
    /// (`"30s"`).
    #[toml_comment(inline)]
    pub intent_lease: HumanDuration,
    /// Base retry delay for a sink rejection, in human-readable form (`"2s"`).
    #[toml_comment(inline)]
    pub intent_retry: HumanDuration,
    /// Ceiling the exponential retry delay is clamped to, in human-readable form
    /// (`"5m"`).
    #[toml_comment(inline)]
    pub intent_retry_max: HumanDuration,
    /// Attempt count from which an intent that keeps failing is reported as stuck.
    ///
    /// There is no dead-letter for a temporal append intent: the intent is the
    /// durable record of history the broker accepted, so it stays claimable
    /// forever and is only reported, never discarded.
    pub intent_stuck_after_attempts: u32,
    /// Override of the span one write-side history partition covers, as a whole
    /// number of days in human-readable form (`"7days"`).
    ///
    /// Absent, the schema's own default stands. Reconciled against the stored
    /// hypertable dimension when a tenant is opened. The partitions are
    /// day-grained, so the value must resolve to whole days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_interval: Option<HumanDuration>,
    /// The age past which a history partition is converted to the columnstore,
    /// as a whole number of days in human-readable form (`"30days"`).
    ///
    /// Enabled and reconciled when a tenant is opened on an edition that has a
    /// columnstore; ignored on one that does not. Must not fall below the chunk
    /// interval, or a partition still receiving rows would be converted.
    #[toml_comment(inline)]
    pub columnstore_after: HumanDuration,
    /// Operator retention horizon as a whole number of days in human-readable
    /// form (`"365days"`), absent while retention is off.
    ///
    /// Retention ages temporal storage out and is not an NGSI-LD operation: the
    /// specification defines deletion of a Temporal Evolution (clause 5.6.16)
    /// and says nothing about storage an operator chooses not to keep. It is
    /// therefore absent unless an operator names a horizon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_horizon: Option<HumanDuration>,
    /// Cadence of the operator retention sweep, in human-readable form (`"1h"`).
    #[toml_comment(inline)]
    pub retention_sweep_interval: HumanDuration,
}

impl Default for TemporalConfig {
    fn default() -> TemporalConfig {
        TemporalConfig {
            append_mode: TemporalAppendMode::Native,
            default_last_n: 100,
            max_last_n: 1000,
            expiration_check_interval: HumanDuration::from_secs(30),
            intent_fallback: HumanDuration::from_secs(1),
            intent_fallback_max: HumanDuration::from_secs(30),
            intent_batch_size: 64,
            intent_lease: HumanDuration::from_secs(30),
            intent_retry: HumanDuration::from_secs(2),
            intent_retry_max: HumanDuration::from_secs(300),
            intent_stuck_after_attempts: 10,
            chunk_interval: None,
            columnstore_after: DEFAULT_COLUMNSTORE_AFTER,
            retention_horizon: None,
            retention_sweep_interval: HumanDuration::from_secs(3600),
        }
    }
}

/// Whether a duration is a positive whole number of days, the grain the
/// history-partition day fields require.
const fn is_positive_whole_days(duration: HumanDuration) -> bool {
    let seconds = duration.duration().as_secs();
    seconds != 0 && seconds.is_multiple_of(SECONDS_PER_DAY)
}

/// The whole days a validated day-grained override spans, as a [`NonZeroU32`].
///
/// Returns [`None`] for a value that is not a positive whole number of days or
/// that overflows [`u32`]; on a configuration that passed
/// [`TemporalConfig::validate`] the day fields always convert.
fn whole_days(duration: HumanDuration) -> Option<NonZeroU32> {
    let days = duration.duration().as_secs() / SECONDS_PER_DAY;
    u32::try_from(days).ok().and_then(NonZeroU32::new)
}

impl TemporalConfig {
    /// Validate temporal caps, drain limits, and cadences.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalConfigError`] variant naming the first setting
    /// that spans no time or no work: a zero default `lastN` page, a `lastN`
    /// maximum below that page or above [`MAX_LAST_N_CEILING`], expiry cadence,
    /// intent fallback cadence, batch size, lease, retry delay, stuck-attempt
    /// threshold, chunk interval, columnstore lag, retention horizon or
    /// retention sweep cadence; a retry ceiling below the retry delay; or a
    /// columnstore lag below the chunk interval, which would convert a
    /// partition that is still being written.
    pub const fn validate(&self) -> Result<(), TemporalConfigError> {
        if self.default_last_n == 0 {
            return Err(TemporalConfigError::ZeroDefaultLastN);
        }
        if self.max_last_n < self.default_last_n {
            return Err(TemporalConfigError::MaxLastNBelowDefault);
        }
        if self.max_last_n > MAX_LAST_N_CEILING {
            return Err(TemporalConfigError::MaxLastNAboveCeiling);
        }
        if self.expiration_check_interval.is_zero() {
            return Err(TemporalConfigError::ZeroExpirationCheckInterval);
        }
        if self.intent_fallback.is_zero() {
            return Err(TemporalConfigError::ZeroIntentFallback);
        }
        if self.intent_fallback_max.duration().as_secs() < self.intent_fallback.duration().as_secs() {
            return Err(TemporalConfigError::IntentFallbackCeilingBelowFloor);
        }
        if self.intent_batch_size == 0 {
            return Err(TemporalConfigError::ZeroIntentBatchSize);
        }
        if self.intent_lease.is_zero() {
            return Err(TemporalConfigError::ZeroIntentLease);
        }
        if self.intent_retry.is_zero() {
            return Err(TemporalConfigError::ZeroIntentRetry);
        }
        if self.intent_retry_max.duration().as_secs() < self.intent_retry.duration().as_secs() {
            return Err(TemporalConfigError::IntentRetryCeilingBelowBase);
        }
        if self.intent_stuck_after_attempts == 0 {
            return Err(TemporalConfigError::ZeroIntentStuckThreshold);
        }
        if let Some(chunk_interval) = self.chunk_interval
            && !is_positive_whole_days(chunk_interval)
        {
            return Err(TemporalConfigError::ChunkIntervalNotWholeDays);
        }
        if !is_positive_whole_days(self.columnstore_after) {
            return Err(TemporalConfigError::ColumnstoreAfterNotWholeDays);
        }
        let chunk_interval = match self.chunk_interval {
            Some(chunk_interval) => chunk_interval,
            None => DEFAULT_CHUNK_INTERVAL,
        };
        if self.columnstore_after.duration().as_secs() < chunk_interval.duration().as_secs() {
            return Err(TemporalConfigError::ColumnstoreLagBelowChunkInterval);
        }
        if let Some(retention_horizon) = self.retention_horizon
            && !is_positive_whole_days(retention_horizon)
        {
            return Err(TemporalConfigError::RetentionHorizonNotWholeDays);
        }
        if self.retention_sweep_interval.is_zero() {
            return Err(TemporalConfigError::ZeroRetentionSweepInterval);
        }
        Ok(())
    }

    /// The page sizes instance-returning temporal reads run under.
    #[must_use]
    pub const fn instance_limits(&self) -> TemporalInstanceLimits {
        TemporalInstanceLimits {
            default_page: self.default_last_n,
            maximum: self.max_last_n,
        }
    }

    /// The storage policy the configuration describes, for the backend to
    /// reconcile.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalConfigError::ChunkIntervalNotWholeDays`] or
    /// [`TemporalConfigError::ColumnstoreAfterNotWholeDays`] for a value that is
    /// not a positive whole number of days, which [`TemporalConfig::validate`]
    /// refuses first on every configuration the broker starts with.
    pub fn storage_policy(&self) -> Result<TemporalStoragePolicy, TemporalConfigError> {
        let chunk_interval_days = match self.chunk_interval {
            Some(chunk_interval) => Some(whole_days(chunk_interval).ok_or(TemporalConfigError::ChunkIntervalNotWholeDays)?),
            None => None,
        };
        Ok(TemporalStoragePolicy {
            chunk_interval_days,
            columnstore_after_days: whole_days(self.columnstore_after).ok_or(TemporalConfigError::ColumnstoreAfterNotWholeDays)?,
        })
    }
}

#[cfg(test)]
mod config_tests {
    use crate::{
        config::{DEFAULT_COLUMNSTORE_AFTER, MAX_LAST_N_CEILING, TemporalAppendMode, TemporalConfig, TemporalConfigError},
        instance_limits::TemporalInstanceLimits,
        storage_policy::TemporalStoragePolicy,
    };
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    fn valid() -> TemporalConfig {
        TemporalConfig {
            append_mode: TemporalAppendMode::Native,
            default_last_n: 19,
            max_last_n: 19,
            expiration_check_interval: HumanDuration::from_secs(30),
            intent_fallback: HumanDuration::from_secs(1),
            intent_fallback_max: HumanDuration::from_secs(30),
            intent_batch_size: 64,
            intent_lease: HumanDuration::from_secs(30),
            intent_retry: HumanDuration::from_secs(2),
            intent_retry_max: HumanDuration::from_secs(300),
            intent_stuck_after_attempts: 10,
            chunk_interval: None,
            columnstore_after: DEFAULT_COLUMNSTORE_AFTER,
            retention_horizon: None,
            retention_sweep_interval: HumanDuration::from_secs(3600),
        }
    }

    /// The shipped broker commits temporal history synchronously inside the
    /// atomic write, so a POST followed by an immediate temporal aggregate GET
    /// reads its own writes. That guarantee holds only in `Native` mode: the
    /// `Deferred` overlay lands history through a sink and would expose a
    /// write-then-read window. This pins the default so flipping it fails loudly
    /// rather than silently reintroducing that staleness.
    #[test]
    fn the_default_append_mode_is_native() {
        assert_eq!(TemporalConfig::default().append_mode, TemporalAppendMode::Native);
        assert_eq!(TemporalAppendMode::default(), TemporalAppendMode::Native);
    }

    /// A read without `lastN` pages at 100 instances per attribute and a client
    /// `lastN` is answered in full up to 1000 unless an operator says otherwise.
    #[test]
    fn the_default_page_is_one_hundred_and_the_maximum_one_thousand() {
        let config = TemporalConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(
            config.instance_limits(),
            TemporalInstanceLimits {
                default_page: 100,
                maximum: 1000
            }
        );
    }

    #[test]
    fn a_zero_default_page_is_rejected() {
        let config = TemporalConfig { default_last_n: 0, ..valid() };
        assert_eq!(config.validate(), Err(TemporalConfigError::ZeroDefaultLastN));
    }

    /// The default page is one a client could ask for, so the maximum may not
    /// sit below it.
    #[test]
    fn a_maximum_below_the_default_page_is_rejected() {
        let config = TemporalConfig {
            default_last_n: 100,
            max_last_n: 99,
            ..valid()
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::MaxLastNBelowDefault));
    }

    #[test]
    fn a_maximum_equal_to_the_default_page_is_accepted() {
        let config = TemporalConfig {
            default_last_n: 19,
            max_last_n: 19,
            ..valid()
        };
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn a_maximum_at_the_ceiling_is_accepted_and_one_above_is_rejected() {
        let at_ceiling = TemporalConfig {
            max_last_n: MAX_LAST_N_CEILING,
            ..valid()
        };
        assert_eq!(at_ceiling.validate(), Ok(()));
        let above = TemporalConfig {
            max_last_n: MAX_LAST_N_CEILING + 1,
            ..valid()
        };
        assert_eq!(above.validate(), Err(TemporalConfigError::MaxLastNAboveCeiling));
    }

    #[test]
    fn a_retry_ceiling_below_the_base_delay_is_rejected() {
        let config = TemporalConfig {
            intent_retry: HumanDuration::from_secs(10),
            intent_retry_max: HumanDuration::from_secs(5),
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
            chunk_interval: Some(HumanDuration::from_days(0)),
            ..valid()
        };
        assert_eq!(chunk.validate(), Err(TemporalConfigError::ChunkIntervalNotWholeDays));
        let lag = TemporalConfig {
            columnstore_after: HumanDuration::from_days(0),
            ..valid()
        };
        assert_eq!(lag.validate(), Err(TemporalConfigError::ColumnstoreAfterNotWholeDays));
    }

    #[test]
    fn a_sub_day_or_fractional_storage_override_is_rejected() {
        // The write-side history partitions are day-grained; a `"36h"` chunk
        // interval has no partition boundary to land on.
        let fractional_chunk = TemporalConfig {
            chunk_interval: Some(HumanDuration::new(Duration::from_hours(36))),
            ..valid()
        };
        assert_eq!(fractional_chunk.validate(), Err(TemporalConfigError::ChunkIntervalNotWholeDays));
        let sub_day_horizon = TemporalConfig {
            retention_horizon: Some(HumanDuration::new(Duration::from_hours(12))),
            ..valid()
        };
        assert_eq!(sub_day_horizon.validate(), Err(TemporalConfigError::RetentionHorizonNotWholeDays));
    }

    #[test]
    fn a_columnstore_lag_shorter_than_the_chunk_interval_is_rejected_against_either_default() {
        let short_lag = TemporalConfig {
            columnstore_after: HumanDuration::from_days(3),
            ..valid()
        };
        assert_eq!(short_lag.validate(), Err(TemporalConfigError::ColumnstoreLagBelowChunkInterval));
        let long_chunk = TemporalConfig {
            chunk_interval: Some(HumanDuration::from_days(60)),
            ..valid()
        };
        assert_eq!(long_chunk.validate(), Err(TemporalConfigError::ColumnstoreLagBelowChunkInterval));
        let both = TemporalConfig {
            chunk_interval: Some(HumanDuration::from_days(1)),
            columnstore_after: HumanDuration::from_days(3),
            ..valid()
        };
        assert_eq!(both.validate(), Ok(()));
    }

    #[test]
    fn the_storage_policy_carries_the_chunk_override_and_the_columnstore_lag() {
        assert_eq!(
            valid().storage_policy(),
            Ok(TemporalStoragePolicy {
                chunk_interval_days: None,
                columnstore_after_days: NonZeroU32::new(30).expect("thirty days"),
            })
        );
        let overridden = TemporalConfig {
            chunk_interval: Some(HumanDuration::from_days(1)),
            columnstore_after: HumanDuration::from_days(3),
            ..valid()
        };
        assert_eq!(
            overridden.storage_policy(),
            Ok(TemporalStoragePolicy {
                chunk_interval_days: NonZeroU32::new(1),
                columnstore_after_days: NonZeroU32::new(3).expect("three days"),
            })
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
        assert_eq!(config.retention_horizon, None);
    }

    #[test]
    fn a_retention_horizon_spanning_no_time_is_rejected() {
        let config = TemporalConfig {
            retention_horizon: Some(HumanDuration::from_days(0)),
            ..valid()
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::RetentionHorizonNotWholeDays));
    }
}
