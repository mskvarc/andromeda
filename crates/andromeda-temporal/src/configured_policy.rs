//! What the temporal machinery takes from the `[temporal]` section.

use crate::{instance_limits::TemporalInstanceLimits, storage_policy::TemporalStoragePolicy};
use andromeda_configuration::schema::temporal::{TemporalConfig, error::TemporalConfigError};
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};

/// The schedule a rejected temporal append intent is retried on: linear from
/// `intent_retry`, clamped at `intent_retry_max`, so an intent that keeps
/// failing is retried at a bounded cadence.
///
/// # Errors
///
/// Returns the [`BackoffScheduleError`] of a zero delay or of a ceiling below
/// it; the configuration loader asks for the schedule, so a broker never
/// starts with one.
pub const fn intent_retry_schedule(config: &TemporalConfig) -> Result<BackoffSchedule, BackoffScheduleError> {
    BackoffSchedule::validated(config.intent_retry.duration(), config.intent_retry_max.duration(), Growth::Linear)
}

impl From<&TemporalConfig> for TemporalInstanceLimits {
    fn from(config: &TemporalConfig) -> TemporalInstanceLimits {
        TemporalInstanceLimits {
            default_page: config.default_last_n,
            maximum: config.max_last_n,
            edge_group: config.max_edge_group,
            aggregation_instances: config.aggregation_instance_budget,
        }
    }
}

impl TryFrom<&TemporalConfig> for TemporalStoragePolicy {
    type Error = TemporalConfigError;

    fn try_from(config: &TemporalConfig) -> Result<TemporalStoragePolicy, TemporalConfigError> {
        Ok(TemporalStoragePolicy {
            chunk_interval_days: config.chunk_interval_days()?,
            columnstore_after_days: config.columnstore_after_days()?,
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
    use andromeda_configuration::{human_duration::HumanDuration, schema::temporal::TemporalConfig};
    use andromeda_retry::schedule_error::BackoffScheduleError;
    use pretty_assertions::assert_eq;
    use std::num::{NonZeroU32, NonZeroU64};

    fn budget(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).expect("a positive budget")
    }

    /// A read without `lastN` pages at 100 instances per attribute and a client
    /// `lastN` is answered in full up to 1000 unless an operator says otherwise.
    #[test]
    fn the_default_page_is_one_hundred_and_the_maximum_one_thousand() {
        let config = TemporalConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(
            TemporalInstanceLimits::from(&config),
            TemporalInstanceLimits {
                default_page: 100,
                maximum: 1000,
                edge_group: 100_000,
                aggregation_instances: budget(1_000_000),
            }
        );
    }

    /// The bound reaches the instance limits the reads are cut under.
    #[test]
    fn the_configured_edge_group_bound_reaches_the_instance_limits() {
        let config = TemporalConfig {
            max_edge_group: 250_000,
            ..TemporalConfig::default()
        };
        assert_eq!(TemporalInstanceLimits::from(&config).edge_group, 250_000);
    }

    /// The budget reaches the instance limits the reads are compiled under.
    #[test]
    fn the_configured_aggregation_instance_budget_reaches_the_instance_limits() {
        let config = TemporalConfig {
            aggregation_instance_budget: budget(50_000),
            ..TemporalConfig::default()
        };
        assert_eq!(TemporalInstanceLimits::from(&config).aggregation_instances, budget(50_000));
    }

    #[test]
    fn a_retry_ceiling_below_the_base_delay_is_rejected() {
        let config = TemporalConfig {
            intent_retry: HumanDuration::from_secs(10),
            intent_retry_max: HumanDuration::from_secs(5),
            ..TemporalConfig::default()
        };
        assert_eq!(intent_retry_schedule(&config).map(drop), Err(BackoffScheduleError::CeilingBelowFirst));
    }

    #[test]
    fn the_storage_policy_carries_the_chunk_override_and_the_columnstore_lag() {
        assert_eq!(
            TemporalStoragePolicy::try_from(&TemporalConfig::default()),
            Ok(TemporalStoragePolicy {
                chunk_interval_days: None,
                columnstore_after_days: NonZeroU32::new(30).expect("thirty days"),
            })
        );
        let overridden = TemporalConfig {
            chunk_interval: Some(HumanDuration::from_days(1)),
            columnstore_after: HumanDuration::from_days(3),
            ..TemporalConfig::default()
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
