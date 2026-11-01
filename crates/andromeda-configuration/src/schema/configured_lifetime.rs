//! A configured span a cached `@context` stays fresh for.

use crate::human_duration::HumanDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// A positive span of at least one second, stated as a cache lifetime.
///
/// Zero is not representable, since a lifetime of nothing caches nothing, and
/// neither is a span below one second, since such a span is advertised as
/// `max-age` in whole seconds, where it would become zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "A cache lifetime of at least one second, since it is advertised as `max-age` in whole seconds.")]
#[serde(try_from = "HumanDuration", into = "HumanDuration")]
pub struct ConfiguredLifetime(Duration);

/// A configured lifetime that cannot be one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ConfiguredLifetimeError {
    /// The span is zero, so nothing would ever be kept.
    #[error("a lifetime must not be zero")]
    Zero,
    /// The span is below one second, which `max-age` cannot state.
    #[error("a lifetime must be at least one second, got {milliseconds} ms")]
    SubSecond {
        /// The configured span, in milliseconds.
        milliseconds: u128,
    },
}

impl ConfiguredLifetime {
    /// The heuristic lifetime of a downloaded `@context` whose response states
    /// none (`jsonld.cache_ttl`): one hour.
    pub const CACHE_TTL_DEFAULT: ConfiguredLifetime = ConfiguredLifetime(Duration::from_secs(3600));

    /// How long an `@context` the broker stores itself is used from memory
    /// before its row is read again (`jsonld.hosted_context_revalidation`): one
    /// minute.
    pub const HOSTED_REVALIDATION_DEFAULT: ConfiguredLifetime = ConfiguredLifetime(Duration::from_secs(60));

    /// The shortest lifetime, one second: `max-age` states whole seconds.
    pub const MINIMUM: Duration = Duration::from_secs(1);

    /// The span.
    #[must_use]
    pub const fn as_duration(self) -> Duration {
        self.0
    }
}

impl TryFrom<HumanDuration> for ConfiguredLifetime {
    type Error = ConfiguredLifetimeError;

    fn try_from(configured: HumanDuration) -> Result<ConfiguredLifetime, ConfiguredLifetimeError> {
        let span = configured.duration();
        if span.is_zero() {
            Err(ConfiguredLifetimeError::Zero)
        } else if span < ConfiguredLifetime::MINIMUM {
            Err(ConfiguredLifetimeError::SubSecond {
                milliseconds: span.as_millis(),
            })
        } else {
            Ok(ConfiguredLifetime(span))
        }
    }
}

impl From<ConfiguredLifetime> for HumanDuration {
    fn from(lifetime: ConfiguredLifetime) -> HumanDuration {
        HumanDuration::new(lifetime.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::configured_lifetime::{ConfiguredLifetime, ConfiguredLifetimeError},
    };
    use std::time::Duration;

    /// A configured lifetime is refused when it is zero or below one second,
    /// and read as given otherwise.
    #[test]
    fn a_configured_lifetime_refuses_zero_and_sub_second_spans() {
        pretty_assertions::assert_eq!(ConfiguredLifetime::try_from(HumanDuration::from_secs(0)), Err(ConfiguredLifetimeError::Zero));
        pretty_assertions::assert_eq!(
            ConfiguredLifetime::try_from(HumanDuration::from_millis(500)),
            Err(ConfiguredLifetimeError::SubSecond { milliseconds: 500 })
        );
        pretty_assertions::assert_eq!(
            ConfiguredLifetime::try_from(HumanDuration::from_secs(1)).map(ConfiguredLifetime::as_duration),
            Ok(Duration::from_secs(1))
        );
    }
}
