//! What entity execution takes from the `[entity]` section.

use andromeda_configuration::schema::entity::EntityConfig;
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::{attribute_nesting::AttributeNestingLimit, payload_budget::EntityPayloadBudget};
use andromeda_retry::{growth::Growth, retry_number::RetryNumber, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use std::num::NonZeroU32;
use thiserror::Error;

/// The retry from which a failed side-effect job's backoff stops doubling.
///
/// Fifteen doublings take the default two-second base past eighteen hours, so
/// a job that keeps failing still comes due again within a day.
pub const PENDING_SIDE_EFFECTS_LAST_GROWING_RETRY: RetryNumber = RetryNumber::new(NonZeroU32::MIN.saturating_add(15));

/// The configured attribute nesting admits attributes deeper than every stored
/// value of one can be read back at.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("entity.max_attribute_nesting ({configured}) must not exceed {ceiling}, the deepest nesting every stored value reads back at")]
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
pub const fn check_attribute_nesting(config: &EntityConfig) -> Result<(), AttributeNestingAboveDecodable> {
    let ceiling = AttributeNestingLimit::DECODABLE_CEILING;
    if config.max_attribute_nesting.get() > ceiling.get() {
        return Err(AttributeNestingAboveDecodable {
            configured: config.max_attribute_nesting.get(),
            ceiling: ceiling.get(),
        });
    }
    Ok(())
}

/// The schedule a failed side-effect job is retried on: doubling from
/// `pending_side_effects_base_backoff` until
/// [`PENDING_SIDE_EFFECTS_LAST_GROWING_RETRY`].
///
/// # Errors
///
/// Returns the [`BackoffScheduleError`] of a zero base backoff; the
/// configuration loader asks for the schedule, so a broker never starts
/// with one.
pub const fn pending_side_effects_retry_schedule(config: &EntityConfig) -> Result<BackoffSchedule, BackoffScheduleError> {
    let first = config.pending_side_effects_base_backoff.duration();
    let ceiling = Growth::Doubling.grow(first, PENDING_SIDE_EFFECTS_LAST_GROWING_RETRY);
    BackoffSchedule::validated(first, ceiling, Growth::Doubling)
}

/// The budget a written entity payload is admitted under.
#[must_use]
pub const fn payload_budget(config: &EntityConfig) -> EntityPayloadBudget {
    EntityPayloadBudget {
        json_values: JsonValueLimit::new(config.max_entity_json_values),
        geometry_positions: GeometryPositionLimit::new(config.max_geometry_positions),
        attribute_nesting: AttributeNestingLimit::new(config.max_attribute_nesting),
    }
}

/// The limits a geometry standing on its own is admitted under: the reference
/// geometry of a query or a subscription, or the geometry of a registration.
#[must_use]
pub const fn geometry_census_limits(config: &EntityConfig) -> GeometryCensusLimits {
    payload_budget(config).geometry_census_limits()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::configured_policy::{AttributeNestingAboveDecodable, check_attribute_nesting, payload_budget, pending_side_effects_retry_schedule};
    use andromeda_configuration::{human_duration::HumanDuration, schema::entity::EntityConfig};
    use andromeda_retry::schedule_error::BackoffScheduleError;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_budgets_admit_one_million_values_and_positions() {
        let budget = payload_budget(&EntityConfig::default());
        assert_eq!(budget.json_values.get(), 1_000_000);
        assert_eq!(budget.geometry_positions.get(), 1_000_000);
    }

    /// An attribute nesting past the depth every stored value reads back at
    /// would admit writes no read can decode; the limit defaults
    /// to 32 and may be raised to that ceiling, not past it.
    #[test]
    fn the_attribute_nesting_defaults_to_32_and_is_refused_past_the_decodable_ceiling() {
        let rendered = toml::to_string(&EntityConfig::default()).expect("the defaults render");
        assert_eq!(payload_budget(&EntityConfig::default()).attribute_nesting.get(), 32);
        let at_ceiling: EntityConfig = toml::from_str(&rendered.replace("max_attribute_nesting = 32", "max_attribute_nesting = 62")).expect("parses");
        assert_eq!(check_attribute_nesting(&at_ceiling), Ok(()));
        let past_ceiling: EntityConfig = toml::from_str(&rendered.replace("max_attribute_nesting = 32", "max_attribute_nesting = 63")).expect("parses");
        assert_eq!(
            check_attribute_nesting(&past_ceiling),
            Err(AttributeNestingAboveDecodable { configured: 63, ceiling: 62 })
        );
        assert!(toml::from_str::<EntityConfig>(&rendered.replace("max_attribute_nesting = 32", "max_attribute_nesting = 0")).is_err());
    }

    #[test]
    fn a_zero_side_effect_backoff_forms_no_retry_schedule() {
        let config = EntityConfig {
            pending_side_effects_base_backoff: HumanDuration::from_secs(0),
            ..EntityConfig::default()
        };
        assert_eq!(pending_side_effects_retry_schedule(&config).map(drop), Err(BackoffScheduleError::ZeroFirst));
    }
}
