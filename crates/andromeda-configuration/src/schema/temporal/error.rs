//! The ways a `[temporal]` table is refused as it is read.

use crate::schema::{
    durable_queue::DurableQueueError,
    temporal::query::{MAX_AGGREGATION_INSTANCES_CEILING, MAX_EDGE_GROUP_CEILING, MAX_LAST_N_CEILING},
};
use thiserror::Error;

/// Invalid temporal configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TemporalConfigError {
    /// The default `lastN` limit was configured as zero.
    #[error("temporal.query.default_last_n must be at least 1")]
    ZeroDefaultLastN,
    /// The `lastN` maximum was configured below the default page.
    ///
    /// A read without `lastN` is paged at the default, and that page has to be
    /// one a client could also ask for explicitly.
    #[error("temporal.query.max_last_n must be at least temporal.query.default_last_n")]
    MaxLastNBelowDefault,
    /// The `lastN` maximum was configured above [`MAX_LAST_N_CEILING`].
    #[error("temporal.query.max_last_n must not exceed {MAX_LAST_N_CEILING}")]
    MaxLastNAboveCeiling,
    /// The edge group bound was configured below the `lastN` maximum.
    ///
    /// A backward page holds up to the maximum instances of one attribute,
    /// all of which may share one instant, so a smaller bound would refuse a
    /// `lastN` the broker answers in full.
    #[error("temporal.query.max_edge_group must be at least temporal.query.max_last_n")]
    MaxEdgeGroupBelowMaxLastN,
    /// The edge group bound was configured above [`MAX_EDGE_GROUP_CEILING`].
    #[error("temporal.query.max_edge_group must not exceed {MAX_EDGE_GROUP_CEILING}")]
    MaxEdgeGroupAboveCeiling,
    /// The aggregation instance bound was configured above
    /// [`MAX_AGGREGATION_INSTANCES_CEILING`].
    #[error("temporal.query.max_aggregation_instances must not exceed {MAX_AGGREGATION_INSTANCES_CEILING}")]
    MaxAggregationInstancesAboveCeiling,
    /// The history queue table is unusable.
    #[error(transparent)]
    HistoryQueue(#[from] DurableQueueError),
    /// The chunk interval override was not a positive whole number of days.
    ///
    /// The write-side history partitions are day-grained, so an override must
    /// resolve to whole days: a fractional or sub-day span such as `"36h"` has
    /// no partition boundary to land on.
    #[error("temporal.storage.chunk_interval must be a positive whole number of days")]
    ChunkIntervalNotWholeDays,
    /// The columnstore lag was not a positive whole number of days.
    #[error("temporal.storage.columnstore.convert_after must be a positive whole number of days")]
    ConvertAfterNotWholeDays,
    /// The columnstore lag would convert a chunk that is still being written.
    #[error("temporal.storage.columnstore.convert_after must be at least temporal.storage.chunk_interval")]
    ConvertAfterBelowChunkInterval,
    /// Columnstore conversion sweep cadence was configured as zero.
    #[error("temporal.storage.columnstore.sweep_interval must be greater than zero")]
    ZeroColumnstoreSweepInterval,
    /// Retention is enabled with no age to delete history at.
    #[error("temporal.storage.retention.delete_after must be set when temporal.storage.retention.mode is \"enabled\"")]
    RetentionWithoutDeleteAfter,
    /// A retention age was not a positive whole number of days.
    #[error("temporal.storage.retention.delete_after must be a positive whole number of days")]
    DeleteAfterNotWholeDays,
    /// Retention sweep cadence was configured as zero.
    #[error("temporal.storage.retention.sweep_interval must be greater than zero")]
    ZeroRetentionSweepInterval,
}

#[cfg(test)]
mod tests {
    use crate::schema::temporal::error::TemporalConfigError;
    use pretty_assertions::assert_eq;

    /// A refusal names the key to correct and the bound it broke.
    #[test]
    fn a_refusal_names_the_key_and_its_bound() {
        assert_eq!(
            TemporalConfigError::MaxEdgeGroupBelowMaxLastN.to_string(),
            "temporal.query.max_edge_group must be at least temporal.query.max_last_n"
        );
        assert_eq!(
            TemporalConfigError::MaxEdgeGroupAboveCeiling.to_string(),
            "temporal.query.max_edge_group must not exceed 1000000"
        );
        assert_eq!(
            TemporalConfigError::MaxLastNAboveCeiling.to_string(),
            "temporal.query.max_last_n must not exceed 5000"
        );
    }
}
