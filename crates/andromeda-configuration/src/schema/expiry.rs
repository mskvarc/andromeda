//! The `[expiry]` table: the per-tenant sweeps that delete what is past its
//! `expiresAt`.

use crate::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid expiry configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ExpiryConfigError {
    /// The sweep cadence was configured as zero.
    #[error("expiry.sweep_interval must be greater than zero")]
    ZeroSweepInterval,
}

/// How often expired resources are deleted, and how many entities one pass
/// reads.
///
/// One cadence drives the sweep of every resource with an `expiresAt`:
/// entities and their attributes (clause 5.2.4), subscriptions (clause
/// 5.8.1.4), context source registrations (clause 5.9.2.4), Snapshots and
/// Entity Maps. A resource may outlive its `expiresAt` in storage by up to one
/// cadence; the read paths hide what has expired in the meantime, so the API
/// never serves it.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExpiryConfig {
    /// How often the sweeps run, in human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub sweep_interval: HumanDuration,
    /// The expired entities, or entities with expired attributes, one pass of
    /// the entity sweep reads at a time.
    #[toml_comment(inline)]
    pub batch_size: NonZeroU32,
}

impl Default for ExpiryConfig {
    fn default() -> ExpiryConfig {
        ExpiryConfig {
            sweep_interval: HumanDuration::from_secs(30),
            batch_size: NonZeroU32::MIN.saturating_add(999),
        }
    }
}

impl ExpiryConfig {
    /// Validate the sweep cadence.
    ///
    /// # Errors
    ///
    /// Returns [`ExpiryConfigError::ZeroSweepInterval`] for a zero cadence.
    pub const fn validate(&self) -> Result<(), ExpiryConfigError> {
        if self.sweep_interval.is_zero() {
            return Err(ExpiryConfigError::ZeroSweepInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::expiry::{ExpiryConfig, ExpiryConfigError},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_sweeps_every_thirty_seconds_a_thousand_at_a_time() {
        let config = ExpiryConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.sweep_interval, HumanDuration::from_secs(30));
        assert_eq!(config.batch_size.get(), 1_000);
    }

    #[test]
    fn a_zero_cadence_is_refused() {
        let config = ExpiryConfig {
            sweep_interval: HumanDuration::from_secs(0),
            ..ExpiryConfig::default()
        };
        assert_eq!(config.validate(), Err(ExpiryConfigError::ZeroSweepInterval));
    }
}
