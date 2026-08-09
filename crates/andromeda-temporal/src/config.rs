//! Temporal policy configuration.

use confique::Config;
use serde::Serialize;

/// Defaults and reconciliation cadence for temporal history.
#[derive(Config, Debug, Clone, Serialize)]
pub struct TemporalConfig {
    /// System default `lastN` cap from clause 6.3.10.
    #[config(env = "ANDROMEDA_TEMPORAL_DEFAULT_LAST_N", default = 19)]
    pub default_last_n: u32,
    /// Interval used by temporal-related expiry reconciliation while retention remains in composition.
    #[config(env = "ANDROMEDA_TEMPORAL_EXPIRATION_CHECK_INTERVAL_SECS", default = 30)]
    pub expiration_check_interval_secs: u64,
}
