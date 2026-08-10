//! Entity spatial-index synchronization configuration.

use confique::Config;
use serde::Serialize;
use thiserror::Error;

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
#[derive(Config, Debug, Clone, Serialize)]
pub struct SpatialConfig {
    /// Interval between full index rebuilds.
    #[config(env = "ANDROMEDA_SPATIAL_REBUILD_INTERVAL_SECS", default = 1800)]
    pub rebuild_interval_secs: u64,
    /// Capacity of the internal entity-change channel.
    #[config(env = "ANDROMEDA_SPATIAL_CHANGE_CHANNEL_CAPACITY", default = 65_536)]
    pub change_channel_capacity: usize,
    /// Minimum interval between aggregated lag warnings.
    #[config(env = "ANDROMEDA_SPATIAL_LAG_LOG_INTERVAL_SECS", default = 5)]
    pub lag_log_interval_secs: u64,
}

impl SpatialConfig {
    /// Validate spatial synchronization cadences and capacity.
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
