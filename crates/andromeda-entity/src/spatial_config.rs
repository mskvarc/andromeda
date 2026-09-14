//! Entity spatial-index synchronization configuration.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid spatial synchronization configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SpatialConfigError {
    /// Reconciliation cadence was configured as zero.
    #[error("spatial.rebuild_interval_secs must be at least 1")]
    ZeroRebuildInterval,
    /// Change-channel capacity was configured as zero.
    #[error("spatial.change_channel_capacity must be at least 1")]
    ZeroChangeChannelCapacity,
    /// Lag-reporting cadence was configured as zero.
    #[error("spatial.lag_log_interval_secs must be at least 1")]
    ZeroLagLogInterval,
}

/// Spatial-index rebuild and change-channel policy.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct SpatialConfig {
    /// Interval between full index rebuilds.
    pub rebuild_interval_secs: u64,
    /// Capacity of the internal entity-change channel.
    pub change_channel_capacity: usize,
    /// Minimum interval between aggregated lag warnings.
    pub lag_log_interval_secs: u64,
}

impl Default for SpatialConfig {
    fn default() -> SpatialConfig {
        SpatialConfig {
            rebuild_interval_secs: 1800,
            change_channel_capacity: 65_536,
            lag_log_interval_secs: 5,
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
        if self.rebuild_interval_secs == 0 {
            return Err(SpatialConfigError::ZeroRebuildInterval);
        }
        if self.change_channel_capacity == 0 {
            return Err(SpatialConfigError::ZeroChangeChannelCapacity);
        }
        if self.lag_log_interval_secs == 0 {
            return Err(SpatialConfigError::ZeroLagLogInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SpatialConfig, SpatialConfigError};

    #[test]
    fn zero_capacity_is_rejected_by_the_owner() {
        let config = SpatialConfig {
            rebuild_interval_secs: 1,
            change_channel_capacity: 0,
            lag_log_interval_secs: 1,
        };
        assert_eq!(config.validate(), Err(SpatialConfigError::ZeroChangeChannelCapacity));
    }
}
