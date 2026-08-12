//! Temporal policy configuration.

use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use thiserror::Error;

/// Invalid temporal policy configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TemporalConfigError {
    /// The default `lastN` limit was configured as zero.
    #[error("temporal.default_last_n must be at least 1")]
    ZeroDefaultLastN,
    /// Expiry reconciliation cadence was configured as zero.
    #[error("temporal.expiration_check_interval_secs must be at least 1")]
    ZeroExpirationCheckInterval,
    /// Intent fallback cadence was configured as zero.
    #[error("temporal.intent_fallback_secs must be at least 1")]
    ZeroIntentFallback,
    /// Intent batch size was configured as zero.
    #[error("temporal.intent_batch_size must be at least 1")]
    ZeroIntentBatchSize,
    /// Intent lease duration was configured as zero.
    #[error("temporal.intent_lease_secs must be at least 1")]
    ZeroIntentLease,
    /// Intent retry delay was configured as zero.
    #[error("temporal.intent_retry_secs must be at least 1")]
    ZeroIntentRetry,
    /// A retention horizon was named but spans no time.
    #[error("temporal.retention_horizon_days must be at least 1 when retention is enabled")]
    ZeroRetentionHorizon,
    /// Retention sweep cadence was configured as zero.
    #[error("temporal.retention_sweep_interval_secs must be at least 1")]
    ZeroRetentionSweepInterval,
}

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
    /// Operator retention horizon in days, absent while retention is off.
    ///
    /// Retention ages temporal storage out and is not an NGSI-LD operation: the
    /// specification defines deletion of a Temporal Evolution (clause 5.6.16)
    /// and says nothing about storage an operator chooses not to keep. It is
    /// therefore absent unless an operator names a horizon.
    #[config(env = "ANDROMEDA_TEMPORAL_RETENTION_HORIZON_DAYS")]
    pub retention_horizon_days: Option<u64>,
    /// Cadence of the operator retention sweep.
    #[config(env = "ANDROMEDA_TEMPORAL_RETENTION_SWEEP_INTERVAL_SECS", default = 3600)]
    pub retention_sweep_interval_secs: u64,
}

impl TemporalConfig {
    /// Validate temporal caps, drain limits, and cadences.
    pub const fn validate(&self) -> Result<(), TemporalConfigError> {
        if self.default_last_n == 0 {
            return Err(TemporalConfigError::ZeroDefaultLastN);
        }
        if self.expiration_check_interval_secs == 0 {
            return Err(TemporalConfigError::ZeroExpirationCheckInterval);
        }
        if self.intent_fallback_secs == 0 {
            return Err(TemporalConfigError::ZeroIntentFallback);
        }
        if self.intent_batch_size == 0 {
            return Err(TemporalConfigError::ZeroIntentBatchSize);
        }
        if self.intent_lease_secs == 0 {
            return Err(TemporalConfigError::ZeroIntentLease);
        }
        if self.intent_retry_secs == 0 {
            return Err(TemporalConfigError::ZeroIntentRetry);
        }
        if let Some(0) = self.retention_horizon_days {
            return Err(TemporalConfigError::ZeroRetentionHorizon);
        }
        if self.retention_sweep_interval_secs == 0 {
            return Err(TemporalConfigError::ZeroRetentionSweepInterval);
        }
        Ok(())
    }
}

#[cfg(test)]
mod config_tests {
    use super::{TemporalAppendMode, TemporalConfig, TemporalConfigError};

    #[test]
    fn zero_intent_batch_size_is_rejected() {
        let config = TemporalConfig {
            append_mode: TemporalAppendMode::Native,
            default_last_n: 19,
            expiration_check_interval_secs: 30,
            intent_fallback_secs: 1,
            intent_batch_size: 0,
            intent_lease_secs: 30,
            intent_retry_secs: 2,
            retention_horizon_days: None,
            retention_sweep_interval_secs: 3600,
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::ZeroIntentBatchSize));
    }

    #[test]
    fn retention_is_off_unless_an_operator_names_a_horizon() {
        let config = TemporalConfig {
            append_mode: TemporalAppendMode::Native,
            default_last_n: 19,
            expiration_check_interval_secs: 30,
            intent_fallback_secs: 1,
            intent_batch_size: 64,
            intent_lease_secs: 30,
            intent_retry_secs: 2,
            retention_horizon_days: None,
            retention_sweep_interval_secs: 3600,
        };
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.retention_horizon_days, None);
    }

    #[test]
    fn a_retention_horizon_spanning_no_time_is_rejected() {
        let config = TemporalConfig {
            append_mode: TemporalAppendMode::Native,
            default_last_n: 19,
            expiration_check_interval_secs: 30,
            intent_fallback_secs: 1,
            intent_batch_size: 64,
            intent_lease_secs: 30,
            intent_retry_secs: 2,
            retention_horizon_days: Some(0),
            retention_sweep_interval_secs: 3600,
        };
        assert_eq!(config.validate(), Err(TemporalConfigError::ZeroRetentionHorizon));
    }
}
