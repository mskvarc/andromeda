//! Entity spatial-index synchronization configuration.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid spatial synchronization configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SpatialConfigError {
    /// Reconciliation cadence was configured as zero.
    #[error("spatial.rebuild_interval must be greater than zero")]
    ZeroRebuildInterval,
    /// Change-channel capacity was configured as zero.
    #[error("spatial.change_channel_capacity must be at least 1")]
    ZeroChangeChannelCapacity,
    /// Lag-reporting cadence was configured as zero.
    #[error("spatial.lag_log_interval must be greater than zero")]
    ZeroLagLogInterval,
}

/// Spatial-index rebuild and change-channel policy.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct SpatialConfig {
    /// Interval between full index rebuilds, in human-readable form (`"30m"`).
    #[toml_comment(inline)]
    pub rebuild_interval: HumanDuration,
    /// Capacity of the internal entity-change channel.
    pub change_channel_capacity: usize,
    /// Minimum interval between aggregated lag warnings, in human-readable form
    /// (`"5s"`).
    #[toml_comment(inline)]
    pub lag_log_interval: HumanDuration,
}

impl Default for SpatialConfig {
    fn default() -> SpatialConfig {
        SpatialConfig {
            rebuild_interval: HumanDuration::from_secs(1800),
            change_channel_capacity: 65_536,
            lag_log_interval: HumanDuration::from_secs(5),
        }
    }
}

impl SpatialConfig {
    /// Validate spatial synchronization cadences and capacity.
    ///
    /// # Errors
    ///
    /// Reports [`SpatialConfigError::ZeroRebuildInterval`],
    /// [`SpatialConfigError::ZeroChangeChannelCapacity`] or
    /// [`SpatialConfigError::ZeroLagLogInterval`] for whichever of the three
    /// limits is configured as zero.
    pub const fn validate(&self) -> Result<(), SpatialConfigError> {
        if self.rebuild_interval.is_zero() {
            return Err(SpatialConfigError::ZeroRebuildInterval);
        }
        if self.change_channel_capacity == 0 {
            return Err(SpatialConfigError::ZeroChangeChannelCapacity);
        }
        if self.lag_log_interval.is_zero() {
            return Err(SpatialConfigError::ZeroLagLogInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SpatialConfig, SpatialConfigError};
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;

    #[test]
    fn zero_capacity_is_rejected_by_the_owner() {
        let config = SpatialConfig {
            rebuild_interval: HumanDuration::from_secs(1),
            change_channel_capacity: 0,
            lag_log_interval: HumanDuration::from_secs(1),
        };
        assert_eq!(config.validate(), Err(SpatialConfigError::ZeroChangeChannelCapacity));
    }
}
