//! Spatial-index synchronization configuration.

use confique::Config;
use serde::Serialize;

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
