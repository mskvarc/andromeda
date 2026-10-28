//! What entity execution takes from the `[request_limits]` and
//! `[entity.side_effects]` tables.

use andromeda_configuration::schema::{entity::side_effects::SideEffectsConfig, request_limits::RequestLimitsConfig};
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_query::filter::complexity::{FilterComplexityLimits, WRITTEN_NESTING_CEILING};
use andromeda_ngsi_ld_representation::{attribute_nesting::AttributeNestingLimit, payload_budget::EntityPayloadBudget};
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use thiserror::Error;

/// The configured attribute nesting admits attributes deeper than every stored
/// value of one can be read back at.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("request_limits.max_attribute_nesting ({configured}) must not exceed {ceiling}, the deepest nesting every stored value reads back at")]
pub struct AttributeNestingAboveDecodable {
    /// The configured limit.
    pub configured: usize,
    /// The deepest nesting every stored value reads back at.
    pub ceiling: usize,
}

/// Check that the configured attribute nesting stays within the depth every
/// stored value reads back at.
///
/// # Errors
///
/// Returns [`AttributeNestingAboveDecodable`] when it passes that depth; the
/// configuration loader asks, so a broker never starts with such a limit.
pub const fn check_attribute_nesting(limits: &RequestLimitsConfig) -> Result<(), AttributeNestingAboveDecodable> {
    let ceiling = AttributeNestingLimit::DECODABLE_CEILING;
    if limits.max_attribute_nesting.get() > ceiling.get() {
        return Err(AttributeNestingAboveDecodable {
            configured: limits.max_attribute_nesting.get(),
            ceiling: ceiling.get(),
        });
    }
    Ok(())
}

/// The configured `q` nesting admits filters nested deeper than the parser
/// admits parentheses at.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("request_limits.max_q_nesting ({configured}) must not exceed {ceiling}, the deepest a q filter's parentheses may be written")]
pub struct FilterNestingAboveParsed {
    /// The configured limit.
    pub configured: usize,
    /// The deepest a filter's parentheses are parsed at.
    pub ceiling: usize,
}

/// Check that the configured `q` nesting stays within the depth a filter's
/// parentheses are parsed at.
///
/// # Errors
///
/// Returns [`FilterNestingAboveParsed`] when it passes that depth; the
/// configuration loader asks, so a broker never starts with such a limit.
pub const fn check_filter_nesting(limits: &RequestLimitsConfig) -> Result<(), FilterNestingAboveParsed> {
    if limits.max_q_nesting.get() > WRITTEN_NESTING_CEILING.get() {
        return Err(FilterNestingAboveParsed {
            configured: limits.max_q_nesting.get(),
            ceiling: WRITTEN_NESTING_CEILING.get(),
        });
    }
    Ok(())
}

/// The limits every `q` filter a request writes is admitted under: an entity
/// or temporal query's, a delete by query's and a subscription's.
#[must_use]
pub const fn filter_complexity_limits(limits: &RequestLimitsConfig) -> FilterComplexityLimits {
    FilterComplexityLimits::new(limits.max_q_comparisons, limits.max_q_nesting)
}

/// The configured side-effect attempt ceiling is above what a job counts.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("entity.side_effects.queue.max_attempts ({configured}) must not exceed {ceiling}, the most attempts a side-effect job counts")]
pub struct SideEffectAttemptsAboveCeiling {
    /// The configured ceiling.
    pub configured: u32,
    /// The most attempts a side-effect job counts.
    pub ceiling: u16,
}

/// The attempts a side-effect job is given before it is dead-lettered.
///
/// # Errors
///
/// Returns [`SideEffectAttemptsAboveCeiling`] when the configured ceiling is
/// above what a job counts; the configuration loader asks, so a broker never
/// starts with one.
pub fn side_effect_max_attempts(config: &SideEffectsConfig) -> Result<u16, SideEffectAttemptsAboveCeiling> {
    let configured = config.queue.max_attempts.get();
    u16::try_from(configured).map_err(|_| SideEffectAttemptsAboveCeiling { configured, ceiling: u16::MAX })
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::configured_policy::{
        AttributeNestingAboveDecodable,
        FilterNestingAboveParsed,
        SideEffectAttemptsAboveCeiling,
        check_attribute_nesting,
        check_filter_nesting,
        filter_complexity_limits,
        payload_budget,
        side_effect_max_attempts,
        side_effect_retry_schedule,
    };
    use andromeda_configuration::{
        human_duration::HumanDuration,
        schema::{entity::side_effects::SideEffectsConfig, request_limits::RequestLimitsConfig},
    };
    use andromeda_retry::schedule_error::BackoffScheduleError;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    #[test]
    fn the_default_budgets_admit_one_million_values_and_positions() {
        let budget = payload_budget(&RequestLimitsConfig::default());
        assert_eq!(budget.json_values.get(), 1_000_000);
        assert_eq!(budget.geometry_positions.get(), 1_000_000);
    }

    /// An attribute nesting past the depth every stored value reads back at
    /// would admit writes no read can decode; the limit defaults to 32 and
    /// may be raised to that ceiling, not past it.
    #[test]
    fn the_attribute_nesting_defaults_to_32_and_is_refused_past_the_decodable_ceiling() {
        let rendered = toml::to_string(&RequestLimitsConfig::default()).expect("the defaults render");
        assert_eq!(payload_budget(&RequestLimitsConfig::default()).attribute_nesting.get(), 32);
        let at_ceiling: RequestLimitsConfig = toml::from_str(&rendered.replace("max_attribute_nesting = 32", "max_attribute_nesting = 62")).expect("parses");
        assert_eq!(check_attribute_nesting(&at_ceiling), Ok(()));
        let past_ceiling: RequestLimitsConfig = toml::from_str(&rendered.replace("max_attribute_nesting = 32", "max_attribute_nesting = 63")).expect("parses");
        assert_eq!(
            check_attribute_nesting(&past_ceiling),
            Err(AttributeNestingAboveDecodable { configured: 63, ceiling: 62 })
        );
        assert!(toml::from_str::<RequestLimitsConfig>(&rendered.replace("max_attribute_nesting = 32", "max_attribute_nesting = 0")).is_err());
    }

    /// A `q` nesting past the depth parentheses are parsed at would name a
    /// limit no filter can reach; it defaults to 16 and may be raised to 64.
    #[test]
    fn the_filter_nesting_defaults_to_16_and_is_refused_past_the_parsed_ceiling() {
        let rendered = toml::to_string(&RequestLimitsConfig::default()).expect("the defaults render");
        let limits = filter_complexity_limits(&RequestLimitsConfig::default());
        assert_eq!((limits.comparisons.get(), limits.nesting.get()), (1000, 16));
        let at_ceiling: RequestLimitsConfig = toml::from_str(&rendered.replace("max_q_nesting = 16", "max_q_nesting = 64")).expect("parses");
        assert_eq!(check_filter_nesting(&at_ceiling), Ok(()));
        let past_ceiling: RequestLimitsConfig = toml::from_str(&rendered.replace("max_q_nesting = 16", "max_q_nesting = 65")).expect("parses");
        assert_eq!(
            check_filter_nesting(&past_ceiling),
            Err(FilterNestingAboveParsed { configured: 65, ceiling: 64 })
        );
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

    #[test]
    fn an_attempt_ceiling_past_what_a_job_counts_is_refused_and_the_largest_kept() {
        let mut config = SideEffectsConfig::default();
        config.queue.max_attempts = NonZeroU32::new(65_536).expect("a positive ceiling");
        assert_eq!(
            side_effect_max_attempts(&config),
            Err(SideEffectAttemptsAboveCeiling {
                configured: 65_536,
                ceiling: u16::MAX,
            })
        );
        config.queue.max_attempts = NonZeroU32::new(65_535).expect("a positive ceiling");
        assert_eq!(side_effect_max_attempts(&config), Ok(u16::MAX));
    }
}
