//! The `[federation.cooldown]` table: how a failing Context Source is held
//! back.

use crate::human_duration::HumanDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How the broker holds back a context source after a failed forwarded
/// request, beside any `management.cooldown` the registration sets (Table
/// 5.2.34-1, clause 6.3.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[schemars(
    description = "How the broker holds back a context source after a failed forwarded request, beside any `management.cooldown` its registration sets."
)]
pub enum CooldownMode {
    /// Only a registration's own `management.cooldown` holds it back.
    #[schemars(description = "Only a registration's own `management.cooldown` holds a source back.")]
    Disabled,
    /// Every failure episode holds the source back for `retry_interval`.
    #[schemars(description = "Every failure holds the source back for `federation.cooldown.retry_interval`.")]
    Constant,
    /// Each failure of an unbroken episode doubles the hold, from
    /// `retry_interval` up to `max_retry_interval`.
    #[schemars(
        description = "Each failure of an unbroken run of failures doubles the hold, from `federation.cooldown.retry_interval` up to `federation.cooldown.max_retry_interval`, with random jitter of up to the hold itself added."
    )]
    Exponential,
}

/// How a failing Context Source is held back.
///
/// The registration's own `management.cooldown` is always a floor.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "How a failing context source is held back. A registration's own `management.cooldown` is always the minimum hold.")]
pub struct CooldownConfig {
    /// How a source is held back after a failed forwarded request:
    /// `disabled` (only the registration's own cooldown), `constant` or
    /// `exponential`.
    #[schemars(
        description = "How a source is held back after a failed forwarded request. The default, `\"disabled\"`, holds back only registrations that set `management.cooldown`."
    )]
    pub mode: CooldownMode,
    /// How long a source is held back after a failure, or after the first
    /// failure of an `exponential` episode, in human-readable form (`"1s"`).
    #[schemars(
        description = "The hold after a failure in `\"constant\"` mode, or after the first failure in `\"exponential\"` mode. Must be above zero unless `federation.cooldown.mode` is `\"disabled\"`."
    )]
    pub retry_interval: HumanDuration,
    /// The longest an `exponential` hold grows to, in human-readable form
    /// (`"1day"`).
    #[schemars(description = "The longest a hold grows to in `\"exponential\"` mode. In that mode it must be at least `federation.cooldown.retry_interval`.")]
    pub max_retry_interval: HumanDuration,
}

impl Default for CooldownConfig {
    fn default() -> CooldownConfig {
        CooldownConfig {
            mode: CooldownMode::Disabled,
            retry_interval: HumanDuration::from_secs(1),
            max_retry_interval: HumanDuration::from_days(1),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::federation::cooldown::{CooldownConfig, CooldownMode};
    use std::time::Duration;

    #[test]
    fn the_default_cooldown_holds_back_only_registrations_that_set_one() {
        let config = CooldownConfig::default();
        pretty_assertions::assert_eq!(config.mode, CooldownMode::Disabled);
        pretty_assertions::assert_eq!(config.retry_interval.duration(), Duration::from_secs(1));
        pretty_assertions::assert_eq!(config.max_retry_interval.duration(), Duration::from_hours(24));
    }

    #[test]
    fn the_cooldown_mode_reads_its_kebab_case_names() {
        #[derive(Debug, serde::Deserialize)]
        struct Probe {
            mode: CooldownMode,
        }
        let read = |name: &str| toml::from_str::<Probe>(&format!("mode = \"{name}\"")).map(|probe| probe.mode);
        pretty_assertions::assert_eq!(read("exponential").expect("exponential"), CooldownMode::Exponential);
        pretty_assertions::assert_eq!(read("constant").expect("constant"), CooldownMode::Constant);
        pretty_assertions::assert_eq!(read("disabled").expect("disabled"), CooldownMode::Disabled);
        assert!(read("off").is_err());
    }
}
