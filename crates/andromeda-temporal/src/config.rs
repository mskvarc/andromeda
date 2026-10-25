//! Temporal policy configuration.

use crate::{
    config_error::TemporalConfigError,
    core_write_history::CoreWriteHistory,
    instance_limits::TemporalInstanceLimits,
    storage_policy::TemporalStoragePolicy,
};
use andromeda_configuration_scalars::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU32, NonZeroU64};
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

/// The largest `temporal.max_edge_group` a deployment may configure.
///
/// A page carries the instances of one attribute sharing the instant at its
/// edge whole, and renders and compacts them in memory before the response is
/// written, so the ceiling bounds the one part of a page no page size bounds.
pub const MAX_EDGE_GROUP_CEILING: u32 = 1_000_000;

/// The largest `temporal.aggregation_instance_budget` a deployment may
/// configure.
///
/// The budget bounds the raw instances one page of an aggregated temporal read
/// aggregates per attribute stream, so the ceiling bounds the rows one request
/// can make the database fold for each stream, as [`MAX_LAST_N_CEILING`] bounds
/// what it can make the broker render.
pub const AGGREGATION_INSTANCE_BUDGET_CEILING: u64 = 100_000_000;

/// The aggregation instance budget the broker reads under when no other is
/// configured.
pub const DEFAULT_AGGREGATION_INSTANCE_BUDGET: NonZeroU64 = NonZeroU64::MIN.saturating_add(999_999);

/// The number of seconds in one day, the grain the history-partition day fields
/// are constrained to.
const SECONDS_PER_DAY: u64 = 86_400;

/// Defaults and reconciliation cadence for temporal history.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct TemporalConfig {
    /// Whether Core API writes add to the Temporal Evolution of the Entities
    /// they change: `"recorded"` (the default) or `"not-recorded"`.
    ///
    /// Under `"recorded"` a Core write records the attribute instances it
    /// modified and nothing for a restatement of what the Entity holds. With
    /// `"not-recorded"` only the Temporal API writes history, for a producer
    /// that publishes its history there alone: a Core write that modifies
    /// current state adds no instance of its own. Core writes still change
    /// current state, and Core queries still see them.
    #[toml_comment(inline)]
    pub core_write_history: CoreWriteHistory,
    /// Instances per attribute a temporal read without `lastN` returns before
    /// it is paged forwards (clause 6.3.10: "If no limits are specified, a
    /// default limit is enforced, depending on implementation specific
    /// configurations").
    ///
    /// Defaults to 100.
    pub default_last_n: u32,
    /// The largest `lastN` answered in full; a larger one is narrowed to this
    /// many instances per attribute and paged backwards with a 206 whose
    /// `Content-Range` still reports the requested `lastN` (clause 6.3.10).
    ///
    /// Defaults to 1000, and may be raised to at most 5000; it must not fall
    /// below `default_last_n`.
    ///
    /// It is also the page of an aggregated read (clause 4.5.19): at most this
    /// many whole periods per attribute, paged forwards with a 206 whose
    /// `Content-Range` ends at the last period served.
    pub max_last_n: u32,
    /// The most instances of one attribute sharing one instant on the
    /// requested axis a temporal page carries.
    ///
    /// A page never splits the instances sharing the instant at its edge,
    /// since a client resumes from a `range-end` `DateTime` that cannot point
    /// inside an instant (clause 6.3.10), so it carries them whole however far
    /// past its size that takes it; one temporal write stamps every instance it
    /// records with one `createdAt` and `modifiedAt`. A group larger than this
    /// is refused with `TooManyResults` (clause 5.5.6). Defaults to 100,000,
    /// and may be raised to at most 1,000,000; it must not fall below
    /// `max_last_n`.
    pub max_edge_group: u32,
    /// The raw instances one page of an aggregated temporal read aggregates
    /// per attribute at most, beside its `max_last_n` periods.
    ///
    /// A page that would aggregate more ends at the start of the period the
    /// first instance beyond the budget falls in; a first period holding more
    /// than the budget alone is read whole, since a period is never split.
    /// Defaults to 1,000,000, and may be raised to at most 100,000,000; zero
    /// is refused as the configuration is read.
    #[toml_comment(inline)]
    pub aggregation_instance_budget: NonZeroU64,
    /// Cadence of every `expiresAt` sweep the broker schedules per tenant:
    /// entities (clause 5.2.4), subscriptions (clause 5.8.1.4), context source
    /// registrations (clause 5.9.2.4), snapshots and entity maps, in
    /// human-readable form (`"30s"`). Each sweep deletes what has expired at the
    /// moment it runs, so a resource may outlive its `expiresAt` in storage by
    /// up to one cadence; the read paths hide an expired registration in the
    /// meantime, so the API never serves one.
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
    /// Maximum intents claimed by one bounded drain pass, which applies them in
    /// one transaction.
    ///
    /// Each statement of an application reads every claimed intent once, so a
    /// wider batch spreads its cost over more entries: 256 applies a
    /// respond-async write of 500 entities in two transactions, while a batch
    /// that fails is applied again one intent per transaction.
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
    /// The broker's own conversion sweep converts every partition that ended
    /// this long ago, on an edition that has a columnstore; ignored on one that
    /// does not. Must not fall below the chunk interval, or a partition still
    /// receiving rows would be converted.
    #[toml_comment(inline)]
    pub columnstore_after: HumanDuration,
    /// Cadence of the columnstore conversion sweep, in human-readable form
    /// (`"1h"`). The first sweep runs when a tenant is opened.
    #[toml_comment(inline)]
    pub columnstore_sweep_interval: HumanDuration,
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
            core_write_history: CoreWriteHistory::Recorded,
            default_last_n: 100,
            max_last_n: 1000,
            max_edge_group: 100_000,
            aggregation_instance_budget: DEFAULT_AGGREGATION_INSTANCE_BUDGET,
            expiration_check_interval: HumanDuration::from_secs(30),
            intent_fallback: HumanDuration::from_secs(1),
            intent_fallback_max: HumanDuration::from_secs(30),
            intent_batch_size: 256,
            intent_lease: HumanDuration::from_secs(30),
            intent_retry: HumanDuration::from_secs(2),
            intent_retry_max: HumanDuration::from_secs(300),
            intent_stuck_after_attempts: 10,
            chunk_interval: None,
            columnstore_after: DEFAULT_COLUMNSTORE_AFTER,
            columnstore_sweep_interval: HumanDuration::from_secs(3600),
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
    /// maximum below that page or above [`MAX_LAST_N_CEILING`], an edge group
    /// bound below that maximum or above [`MAX_EDGE_GROUP_CEILING`], an
    /// aggregation instance budget above
    /// [`AGGREGATION_INSTANCE_BUDGET_CEILING`], expiry cadence,
    /// intent fallback cadence, batch size, lease, retry delay, stuck-attempt
    /// threshold, chunk interval, columnstore lag, retention horizon,
    /// retention sweep cadence or columnstore sweep cadence; a retry ceiling below the retry delay; or a
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
        if self.max_edge_group < self.max_last_n {
            return Err(TemporalConfigError::MaxEdgeGroupBelowMaxLastN);
        }
        if self.max_edge_group > MAX_EDGE_GROUP_CEILING {
            return Err(TemporalConfigError::MaxEdgeGroupAboveCeiling);
        }
        if self.aggregation_instance_budget.get() > AGGREGATION_INSTANCE_BUDGET_CEILING {
            return Err(TemporalConfigError::AggregationInstanceBudgetAboveCeiling);
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
        if self.columnstore_sweep_interval.is_zero() {
            return Err(TemporalConfigError::ZeroColumnstoreSweepInterval);
        }
        Ok(())
    }

    /// The page sizes instance-returning temporal reads run under.
    #[must_use]
    pub const fn instance_limits(&self) -> TemporalInstanceLimits {
        TemporalInstanceLimits {
            default_page: self.default_last_n,
            maximum: self.max_last_n,
            edge_group: self.max_edge_group,
            aggregation_instances: self.aggregation_instance_budget,
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod config_tests {
    use crate::{
        config::{
            AGGREGATION_INSTANCE_BUDGET_CEILING,
            DEFAULT_AGGREGATION_INSTANCE_BUDGET,
            DEFAULT_COLUMNSTORE_AFTER,
            MAX_EDGE_GROUP_CEILING,
            MAX_LAST_N_CEILING,
            TemporalConfig,
        },
        config_error::TemporalConfigError,
        core_write_history::CoreWriteHistory,
        instance_limits::TemporalInstanceLimits,
        storage_policy::TemporalStoragePolicy,
    };
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;
    use std::{
        num::{NonZeroU32, NonZeroU64},
        time::Duration,
    };

    fn valid() -> TemporalConfig {
        TemporalConfig {
            core_write_history: CoreWriteHistory::Recorded,
            default_last_n: 19,
            max_last_n: 19,
            max_edge_group: 100_000,
            aggregation_instance_budget: DEFAULT_AGGREGATION_INSTANCE_BUDGET,
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
            columnstore_sweep_interval: HumanDuration::from_secs(3600),
            retention_horizon: None,
            retention_sweep_interval: HumanDuration::from_secs(3600),
        }
    }

    #[test]
    fn the_columnstore_sweep_runs_hourly_unless_configured_and_never_at_a_zero_cadence() {
        assert_eq!(TemporalConfig::default().columnstore_sweep_interval.duration(), Duration::from_secs(3600));
        let zero = TemporalConfig {
            columnstore_sweep_interval: HumanDuration::from_secs(0),
            ..valid()
        };
        assert_eq!(zero.validate(), Err(TemporalConfigError::ZeroColumnstoreSweepInterval));
    }

    /// Core writes are recorded in history unless an operator turns that off,
    /// and the setting reads from its kebab-case configuration value.
    #[test]
    fn core_writes_are_recorded_by_default_and_the_setting_reads_from_configuration() {
        assert_eq!(TemporalConfig::default().core_write_history, CoreWriteHistory::Recorded);
        let mut document = serde_json::to_value(TemporalConfig::default()).expect("the default configuration serialises");
        document["core_write_history"] = serde_json::json!("not-recorded");
        let configured: TemporalConfig = serde_json::from_value(document).expect("the configuration deserialises");
        assert_eq!(configured.core_write_history, CoreWriteHistory::NotRecorded);
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
                maximum: 1000,
                edge_group: 100_000,
                aggregation_instances: budget(1_000_000),
            }
        );
    }

    /// A page carries an edge group of up to a hundred thousand instances of
    /// one attribute unless an operator says otherwise.
    #[test]
    fn the_edge_group_bound_is_one_hundred_thousand() {
        assert_eq!(TemporalConfig::default().max_edge_group, 100_000);
        assert_eq!(TemporalConfig::default().validate(), Ok(()));
    }

    /// A backward page may hold the maximum instances at one instant, so the
    /// bound may equal the maximum and not fall below it.
    #[test]
    fn an_edge_group_bound_below_the_maximum_is_rejected() {
        let below = TemporalConfig {
            max_last_n: 1000,
            max_edge_group: 999,
            ..valid()
        };
        assert_eq!(below.validate(), Err(TemporalConfigError::MaxEdgeGroupBelowMaxLastN));
        let equal = TemporalConfig {
            max_last_n: 1000,
            max_edge_group: 1000,
            ..valid()
        };
        assert_eq!(equal.validate(), Ok(()));
    }

    #[test]
    fn an_edge_group_bound_at_the_ceiling_is_accepted_and_one_above_is_rejected() {
        let at_ceiling = TemporalConfig {
            max_edge_group: MAX_EDGE_GROUP_CEILING,
            ..valid()
        };
        assert_eq!(at_ceiling.validate(), Ok(()));
        let above = TemporalConfig {
            max_edge_group: MAX_EDGE_GROUP_CEILING + 1,
            ..valid()
        };
        assert_eq!(above.validate(), Err(TemporalConfigError::MaxEdgeGroupAboveCeiling));
    }

    /// The bound reaches the instance limits the reads are cut under.
    #[test]
    fn the_configured_edge_group_bound_reaches_the_instance_limits() {
        let config = TemporalConfig {
            max_edge_group: 250_000,
            ..valid()
        };
        assert_eq!(config.instance_limits().edge_group, 250_000);
    }

    /// `n` as an aggregation instance budget.
    fn budget(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).expect("a positive budget")
    }

    /// An aggregated page aggregates at most a million raw instances per
    /// attribute unless an operator says otherwise.
    #[test]
    fn the_aggregation_instance_budget_is_one_million() {
        assert_eq!(TemporalConfig::default().aggregation_instance_budget, budget(1_000_000));
        assert_eq!(TemporalConfig::default().validate(), Ok(()));
    }

    /// A page always holds a whole period, so a budget of no instances
    /// describes no page at all and the configuration is refused as it is
    /// read.
    #[test]
    fn a_zero_aggregation_instance_budget_is_refused_as_the_configuration_is_read() {
        let mut document = serde_json::to_value(TemporalConfig::default()).expect("the default configuration serialises");
        document["aggregation_instance_budget"] = serde_json::json!(0);
        assert!(serde_json::from_value::<TemporalConfig>(document).is_err());
    }

    #[test]
    fn an_aggregation_instance_budget_at_the_ceiling_is_accepted_and_one_above_is_rejected() {
        let at_ceiling = TemporalConfig {
            aggregation_instance_budget: budget(AGGREGATION_INSTANCE_BUDGET_CEILING),
            ..valid()
        };
        assert_eq!(at_ceiling.validate(), Ok(()));
        let above = TemporalConfig {
            aggregation_instance_budget: budget(AGGREGATION_INSTANCE_BUDGET_CEILING + 1),
            ..valid()
        };
        assert_eq!(above.validate(), Err(TemporalConfigError::AggregationInstanceBudgetAboveCeiling));
    }

    /// The budget reaches the instance limits the reads are compiled under.
    #[test]
    fn the_configured_aggregation_instance_budget_reaches_the_instance_limits() {
        let config = TemporalConfig {
            aggregation_instance_budget: budget(50_000),
            ..valid()
        };
        assert_eq!(config.instance_limits().aggregation_instances, budget(50_000));
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

    /// A respond-async write of 500 entities drains in two batches.
    #[test]
    fn the_drain_applies_256_intents_per_batch_by_default() {
        assert_eq!(TemporalConfig::default().intent_batch_size, 256);
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
