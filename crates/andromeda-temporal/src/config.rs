//! Temporal policy configuration.

use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How the PostgreSQL anchor lands temporal append consequences.
#[derive(Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum TemporalAppendMode {
    /// Persist temporal history inside the anchor transaction.
    #[default]
    Native,
    /// Persist a typed intent in the anchor and deliver it through the selected sink.
    Deferred,
}

/// Defaults and reconciliation cadence for temporal history.
#[derive(Config, Debug, Clone, Serialize)]
pub struct TemporalConfig {
    /// Native or deferred append topology selected for this broker profile.
    #[config(env = "ANDROMEDA_TEMPORAL_APPEND_MODE", default = "native")]
    pub append_mode: TemporalAppendMode,
    /// System default `lastN` cap from clause 6.3.10.
    #[config(env = "ANDROMEDA_TEMPORAL_DEFAULT_LAST_N", default = 19)]
    pub default_last_n: u32,
    /// Interval used by temporal-related expiry reconciliation while retention remains in composition.
    #[config(env = "ANDROMEDA_TEMPORAL_EXPIRATION_CHECK_INTERVAL_SECS", default = 30)]
    pub expiration_check_interval_secs: u64,
    /// Fallback poll cadence for committed temporal intents.
    #[config(env = "ANDROMEDA_TEMPORAL_INTENT_FALLBACK_SECS", default = 1)]
    pub intent_fallback_secs: u64,
    /// Maximum intents claimed by one bounded drain pass.
    #[config(env = "ANDROMEDA_TEMPORAL_INTENT_BATCH_SIZE", default = 64)]
    pub intent_batch_size: u32,
    /// Lease duration for one claimed intent batch.
    #[config(env = "ANDROMEDA_TEMPORAL_INTENT_LEASE_SECS", default = 30)]
    pub intent_lease_secs: u64,
    /// Base retry delay for a sink rejection.
    #[config(env = "ANDROMEDA_TEMPORAL_INTENT_RETRY_SECS", default = 2)]
    pub intent_retry_secs: u64,
}
