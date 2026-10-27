//! What the temporal machinery takes from the `[temporal]` tables.

use crate::{instance_limits::TemporalInstanceLimits, storage_policy::TemporalStoragePolicy};
use andromeda_configuration::schema::temporal::{
    history::TemporalHistoryConfig,
    query::TemporalQueryConfig,
    storage::{StorageDaysError, TemporalStorageConfig},
};
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};

/// The schedule a rejected temporal history intent is retried on: linear from
/// `retry_interval`, clamped at `max_retry_interval`, so an intent that keeps
/// failing is retried at a bounded cadence.
///
/// # Errors
///
/// Returns the [`BackoffScheduleError`] of a zero delay or of a ceiling below
/// it; the configuration loader asks for the schedule, so a broker never
/// starts with one.
pub const fn intent_retry_schedule(config: &TemporalHistoryConfig) -> Result<BackoffSchedule, BackoffScheduleError> {
    BackoffSchedule::validated(
        config.queue.retry_interval.duration(),
        config.queue.max_retry_interval.duration(),
        Growth::Linear,
    )
}

impl From<&TemporalQueryConfig> for TemporalInstanceLimits {
    fn from(config: &TemporalQueryConfig) -> TemporalInstanceLimits {
        TemporalInstanceLimits {
            default_page: config.default_last_n,
            maximum: config.max_last_n,
            edge_group: config.max_edge_group,
            aggregation_instances: config.max_aggregation_instances,
        }
    }
}

impl TryFrom<&TemporalStorageConfig> for TemporalStoragePolicy {
    type Error = StorageDaysError;

    fn try_from(config: &TemporalStorageConfig) -> Result<TemporalStoragePolicy, StorageDaysError> {
        Ok(TemporalStoragePolicy {
            chunk_interval_days: config.chunk_interval_days()?,
            columnstore_after_days: config.convert_after_days()?,
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{configured_policy::intent_retry_schedule, instance_limits::TemporalInstanceLimits, storage_policy::TemporalStoragePolicy};
    use andromeda_configuration::{
        human_duration::HumanDuration,
        schema::temporal::{
            history::TemporalHistoryConfig,
            query::TemporalQueryConfig,
            storage::{ColumnstoreConfig, TemporalStorageConfig},
        },
    };
    use andromeda_retry::schedule_error::BackoffScheduleError;
    use pretty_assertions::assert_eq;
    use std::num::{NonZeroU32, NonZeroU64};

    fn bound(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).expect("a positive bound")
    }

    /// A read without `lastN` pages at 100 instances per attribute and a client
    /// `lastN` is answered in full up to 1000 unless an operator says otherwise.
    #[test]
    fn the_default_page_is_one_hundred_and_the_maximum_one_thousand() {
        assert_eq!(
            TemporalInstanceLimits::from(&TemporalQueryConfig::default()),
            TemporalInstanceLimits {
                default_page: 100,
                maximum: 1000,
                edge_group: 100_000,
                aggregation_instances: bound(1_000_000),
            }
        );
    }

    /// The configured bounds reach the instance limits the reads are cut under.
    #[test]
    fn the_configured_bounds_reach_the_instance_limits() {
        let config = TemporalQueryConfig {
            max_edge_group: 250_000,
            max_aggregation_instances: bound(50_000),
            ..TemporalQueryConfig::default()
        };
        let limits = TemporalInstanceLimits::from(&config);
        assert_eq!(limits.edge_group, 250_000);
        assert_eq!(limits.aggregation_instances, bound(50_000));
    }

    #[test]
    fn a_retry_ceiling_below_the_base_delay_is_rejected() {
        let mut config = TemporalHistoryConfig::default();
        config.queue.retry_interval = HumanDuration::from_secs(10);
        config.queue.max_retry_interval = HumanDuration::from_secs(5);
        assert_eq!(intent_retry_schedule(&config).map(drop), Err(BackoffScheduleError::CeilingBelowFirst));
    }

    #[test]
    fn the_storage_policy_carries_the_chunk_override_and_the_columnstore_lag() {
        assert_eq!(
            TemporalStoragePolicy::try_from(&TemporalStorageConfig::default()),
            Ok(TemporalStoragePolicy {
                chunk_interval_days: None,
                columnstore_after_days: NonZeroU32::new(30).expect("thirty days"),
            })
        );
        let overridden = TemporalStorageConfig {
            chunk_interval: Some(HumanDuration::from_days(1)),
            columnstore: ColumnstoreConfig {
                convert_after: HumanDuration::from_days(3),
                ..ColumnstoreConfig::default()
            },
            ..TemporalStorageConfig::default()
        };
        assert_eq!(
            TemporalStoragePolicy::try_from(&overridden),
            Ok(TemporalStoragePolicy {
                chunk_interval_days: NonZeroU32::new(1),
                columnstore_after_days: NonZeroU32::new(3).expect("three days"),
            })
        );
    }
}
