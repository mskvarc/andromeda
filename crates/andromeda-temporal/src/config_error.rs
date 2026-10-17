//! The ways a temporal policy configuration is refused as it is read.

use crate::config::{AGGREGATION_INSTANCE_BUDGET_CEILING, MAX_EDGE_GROUP_CEILING, MAX_LAST_N_CEILING};
use thiserror::Error;

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
    /// The edge group bound was configured below the `lastN` maximum.
    ///
    /// A backward page holds up to the maximum instances of one attribute,
    /// all of which may share one instant, so a smaller bound would refuse a
    /// `lastN` the broker answers in full.
    #[error("temporal.max_edge_group must be at least temporal.max_last_n")]
    MaxEdgeGroupBelowMaxLastN,
    /// The edge group bound was configured above [`MAX_EDGE_GROUP_CEILING`].
    #[error("temporal.max_edge_group must not exceed {MAX_EDGE_GROUP_CEILING}")]
    MaxEdgeGroupAboveCeiling,
    /// The aggregation instance budget was configured above
    /// [`AGGREGATION_INSTANCE_BUDGET_CEILING`].
    #[error("temporal.aggregation_instance_budget must not exceed {AGGREGATION_INSTANCE_BUDGET_CEILING}")]
    AggregationInstanceBudgetAboveCeiling,
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
    /// Columnstore conversion sweep cadence was configured as zero.
    #[error("temporal.columnstore_sweep_interval must be greater than zero")]
    ZeroColumnstoreSweepInterval,
}

#[cfg(test)]
mod tests {
    use crate::config_error::TemporalConfigError;
    use pretty_assertions::assert_eq;

    /// A refusal names the setting to correct and the bound it broke.
    #[test]
    fn a_refusal_names_the_setting_and_its_bound() {
        assert_eq!(
            TemporalConfigError::MaxEdgeGroupBelowMaxLastN.to_string(),
            "temporal.max_edge_group must be at least temporal.max_last_n"
        );
        assert_eq!(
            TemporalConfigError::MaxEdgeGroupAboveCeiling.to_string(),
            "temporal.max_edge_group must not exceed 1000000"
        );
        assert_eq!(
            TemporalConfigError::MaxLastNAboveCeiling.to_string(),
            "temporal.max_last_n must not exceed 5000"
        );
    }
}
