//! What entity execution takes from the `[request_limits]` and
//! `[entity.side_effects]` tables.

use andromeda_configuration::schema::{entity::side_effects::SideEffectsConfig, request_limits::RequestLimitsConfig};
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_query::filter::complexity::FilterComplexityLimits;
use andromeda_ngsi_ld_representation::{attribute_nesting::AttributeNestingLimit, payload_budget::EntityPayloadBudget};
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};

/// The limits every `q` filter a request writes is admitted under: an entity
/// or temporal query's, a delete by query's and a subscription's.
#[must_use]
pub const fn filter_complexity_limits(limits: &RequestLimitsConfig) -> FilterComplexityLimits {
    FilterComplexityLimits::new(limits.max_q_comparisons, limits.max_q_nesting)
}

/// The schedule a failed side-effect job is retried on: doubling from
/// `retry_interval` up to `max_retry_interval`.
///
/// # Errors
///
/// Returns the [`BackoffScheduleError`] of a zero first delay or a ceiling
/// below it; the configuration loader asks for the schedule, so a broker
/// never starts with one.
pub const fn side_effect_retry_schedule(config: &SideEffectsConfig) -> Result<BackoffSchedule, BackoffScheduleError> {
    BackoffSchedule::validated(
        config.queue.retry_interval.duration(),
        config.queue.max_retry_interval.duration(),
        Growth::Doubling,
    )
}

/// The budget a written entity payload is admitted under.
#[must_use]
pub const fn payload_budget(limits: &RequestLimitsConfig) -> EntityPayloadBudget {
    EntityPayloadBudget {
        json_values: JsonValueLimit::new(limits.max_entity_json_values),
        geometry_positions: GeometryPositionLimit::new(limits.max_geometry_positions),
        attribute_nesting: AttributeNestingLimit::new(limits.max_attribute_nesting),
    }
}

/// The limits a geometry standing on its own is admitted under: the reference
/// geometry of a query or a subscription, or the geometry of a registration.
#[must_use]
pub const fn geometry_census_limits(limits: &RequestLimitsConfig) -> GeometryCensusLimits {
    payload_budget(limits).geometry_census_limits()
}

#[cfg(test)]
mod tests {
    use crate::configured_policy::{filter_complexity_limits, payload_budget, side_effect_retry_schedule};
    use andromeda_configuration::{
        human_duration::HumanDuration,
        schema::{
            entity::side_effects::SideEffectsConfig,
            request_limits::{MAX_ATTRIBUTE_NESTING_CEILING, MAX_Q_NESTING_CEILING, RequestLimitsConfig},
        },
    };
    use andromeda_ngsi_ld_query::filter::complexity::WRITTEN_NESTING_CEILING;
    use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
    use andromeda_retry::schedule_error::BackoffScheduleError;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_budgets_admit_one_million_values_and_positions() {
        let budget = payload_budget(&RequestLimitsConfig::default());
        assert_eq!(budget.json_values.get(), 1_000_000);
        assert_eq!(budget.geometry_positions.get(), 1_000_000);
    }

    #[test]
    fn the_nesting_limits_default_to_32_attribute_levels_and_16_filter_levels() {
        assert_eq!(payload_budget(&RequestLimitsConfig::default()).attribute_nesting.get(), 32);
        let limits = filter_complexity_limits(&RequestLimitsConfig::default());
        assert_eq!((limits.comparisons.get(), limits.nesting.get()), (1000, 16));
    }

    /// The configuration refuses a nesting limit past its declared ceiling, so
    /// each ceiling is the depth the representation and the filter parser
    /// themselves stop at.
    #[test]
    fn the_nesting_ceilings_the_configuration_declares_are_the_ones_reads_and_parsing_stop_at() {
        assert_eq!(MAX_ATTRIBUTE_NESTING_CEILING, AttributeNestingLimit::DECODABLE_CEILING.get());
        assert_eq!(MAX_Q_NESTING_CEILING, WRITTEN_NESTING_CEILING.get());
    }

    #[test]
    fn a_zero_side_effect_retry_interval_forms_no_retry_schedule() {
        let mut config = SideEffectsConfig::default();
        config.queue.retry_interval = HumanDuration::from_secs(0);
        assert_eq!(side_effect_retry_schedule(&config).map(drop), Err(BackoffScheduleError::ZeroFirst));
    }

    #[test]
    fn the_default_side_effect_retries_double_from_two_seconds_to_five_minutes() {
        assert!(side_effect_retry_schedule(&SideEffectsConfig::default()).is_ok());
    }
}
