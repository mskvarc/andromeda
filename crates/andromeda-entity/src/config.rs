//! Entity execution and background-operation configuration.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid entity execution configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EntityConfigError {
    /// Batch concurrency was explicitly configured as zero.
    #[error("entity.batch_concurrency must be at least 1")]
    ZeroBatchConcurrency,
    /// Batch preparation concurrency was explicitly configured as zero.
    #[error("entity.batch_chunk_concurrency must be at least 1")]
    ZeroBatchChunkConcurrency,
    /// Batch write concurrency was explicitly configured as zero.
    #[error("entity.batch_chunk_write_concurrency must be at least 1")]
    ZeroBatchChunkWriteConcurrency,
    /// Batch chunk size was configured as zero.
    #[error("entity.batch_chunk_size must be at least 1")]
    ZeroBatchChunkSize,
    /// The large-batch threshold was configured as zero.
    #[error("entity.batch_large_threshold must be at least 1")]
    ZeroBatchLargeThreshold,
    /// The blocking preparation threshold was configured as zero.
    #[error("entity.batch_prepare_blocking_threshold must be at least 1")]
    ZeroBatchPrepareBlockingThreshold,
    /// The asynchronous side-effect threshold was explicitly configured as zero.
    #[error("entity.batch_async_side_effect_threshold must be at least 1")]
    ZeroBatchAsyncSideEffectThreshold,
    /// Entity expiry batch size was configured as zero.
    #[error("entity.expiry_batch_size must be at least 1")]
    ZeroExpiryBatchSize,
    /// The expiry batch size cannot be represented by the expiry source port.
    #[error("entity.expiry_batch_size must fit in an unsigned 32-bit integer")]
    ExpiryBatchSizeOutOfRange,
    /// Pending side-effect polling cadence was configured as zero.
    #[error("entity.pending_side_effects_poll_secs must be at least 1")]
    ZeroPendingSideEffectsPollCadence,
    /// The idle side-effect polling ceiling was configured below the polling floor.
    #[error("entity.pending_side_effects_poll_max_secs must be at least entity.pending_side_effects_poll_secs")]
    PendingSideEffectsPollCeilingBelowFloor,
    /// Pending side-effect lease duration was configured as zero.
    #[error("entity.pending_side_effects_lease_secs must be at least 1")]
    ZeroPendingSideEffectsLeaseDuration,
    /// Pending side-effect batch size was configured as zero.
    #[error("entity.pending_side_effects_batch_size must be at least 1")]
    ZeroPendingSideEffectsBatchSize,
    /// Pending side-effect attempt limit was configured as zero.
    #[error("entity.pending_side_effects_max_attempts must be at least 1")]
    ZeroPendingSideEffectsMaxAttempts,
    /// Pending side-effect backoff was configured as zero.
    #[error("entity.pending_side_effects_base_backoff_secs must be at least 1")]
    ZeroPendingSideEffectsBaseBackoff,
    /// Dead-lettered side-effect retention was configured as zero.
    #[error("entity.pending_side_effects_dead_letter_after_secs must be at least 1")]
    ZeroPendingSideEffectsDeadLetterRetention,
    /// Dead-lettered side-effect reap cadence was configured as zero.
    #[error("entity.pending_side_effects_cleanup_interval_secs must be at least 1")]
    ZeroPendingSideEffectsCleanupInterval,
}

/// Entity batching, expiry, and durable side-effect configuration.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct EntityConfig {
    /// Maximum concurrent entities processed in batch operations.
    pub batch_concurrency: Option<usize>,
    /// Maximum concurrent CPU-bound preparation chunks.
    pub batch_chunk_concurrency: Option<usize>,
    /// Maximum concurrent database write chunks.
    pub batch_chunk_write_concurrency: Option<usize>,
    /// Number of entities processed in one batch chunk.
    pub batch_chunk_size: usize,
    /// Entity threshold selecting the large-batch path.
    pub batch_large_threshold: usize,
    /// Entity threshold selecting blocking-pool preparation.
    pub batch_prepare_blocking_threshold: usize,
    /// Minimum batch size eligible for asynchronous side effects.
    pub batch_async_side_effect_threshold: Option<usize>,
    /// Maximum entity or attribute-expiry candidates read per sweep phase.
    pub expiry_batch_size: usize,
    /// Poll interval for durable pending side effects: the floor of the drain's
    /// idle backoff.
    ///
    /// After a commit's edge wake the drain runs promptly; while idle it backs
    /// off from here toward `pending_side_effects_poll_max_secs`.
    pub pending_side_effects_poll_secs: u64,
    /// Ceiling the side-effect drain's idle backoff reaches while no commit wakes it.
    pub pending_side_effects_poll_max_secs: u64,
    /// Lease duration for a claimed side-effect job.
    pub pending_side_effects_lease_secs: u64,
    /// Maximum jobs claimed per poll.
    pub pending_side_effects_batch_size: usize,
    /// Attempts before a side-effect job is dead-lettered.
    pub pending_side_effects_max_attempts: u16,
    /// Base retry backoff for side-effect jobs.
    pub pending_side_effects_base_backoff_secs: u64,
    /// Age after which a dead-lettered side-effect job is removed.
    ///
    /// A dead job is evidence of a batch whose deferred side effects never
    /// landed, so it is kept for a week by default before the reap removes it.
    pub pending_side_effects_dead_letter_after_secs: u64,
    /// Cadence of the dead-lettered side-effect reap.
    pub pending_side_effects_cleanup_interval_secs: u64,
}

impl Default for EntityConfig {
    fn default() -> EntityConfig {
        EntityConfig {
            batch_concurrency: None,
            batch_chunk_concurrency: None,
            batch_chunk_write_concurrency: None,
            batch_chunk_size: 512,
            batch_large_threshold: 512,
            batch_prepare_blocking_threshold: 128,
            batch_async_side_effect_threshold: None,
            expiry_batch_size: 1_000,
            pending_side_effects_poll_secs: 1,
            pending_side_effects_poll_max_secs: 30,
            pending_side_effects_lease_secs: 30,
            pending_side_effects_batch_size: 32,
            pending_side_effects_max_attempts: 5,
            pending_side_effects_base_backoff_secs: 2,
            pending_side_effects_dead_letter_after_secs: 604_800,
            pending_side_effects_cleanup_interval_secs: 300,
        }
    }
}

impl EntityConfig {
    /// Validate the entity policy and execution limits.
    ///
    /// # Errors
    ///
    /// Reports the matching [`EntityConfigError`] variant for the first limit
    /// that is zero, and [`EntityConfigError::ExpiryBatchSizeOutOfRange`] when
    /// the expiry batch size does not fit the unsigned 32-bit width the expiry
    /// source port accepts.
    pub fn validate(&self) -> Result<(), EntityConfigError> {
        if self.batch_concurrency == Some(0) {
            return Err(EntityConfigError::ZeroBatchConcurrency);
        }
        if self.batch_chunk_concurrency == Some(0) {
            return Err(EntityConfigError::ZeroBatchChunkConcurrency);
        }
        if self.batch_chunk_write_concurrency == Some(0) {
            return Err(EntityConfigError::ZeroBatchChunkWriteConcurrency);
        }
        if self.batch_chunk_size == 0 {
            return Err(EntityConfigError::ZeroBatchChunkSize);
        }
        if self.batch_large_threshold == 0 {
            return Err(EntityConfigError::ZeroBatchLargeThreshold);
        }
        if self.batch_prepare_blocking_threshold == 0 {
            return Err(EntityConfigError::ZeroBatchPrepareBlockingThreshold);
        }
        if self.batch_async_side_effect_threshold == Some(0) {
            return Err(EntityConfigError::ZeroBatchAsyncSideEffectThreshold);
        }
        if self.expiry_batch_size == 0 {
            return Err(EntityConfigError::ZeroExpiryBatchSize);
        }
        if self.pending_side_effects_poll_secs == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsPollCadence);
        }
        if self.pending_side_effects_poll_max_secs < self.pending_side_effects_poll_secs {
            return Err(EntityConfigError::PendingSideEffectsPollCeilingBelowFloor);
        }
        if self.pending_side_effects_lease_secs == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsLeaseDuration);
        }
        if self.pending_side_effects_batch_size == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsBatchSize);
        }
        if self.pending_side_effects_max_attempts == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsMaxAttempts);
        }
        if self.pending_side_effects_base_backoff_secs == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsBaseBackoff);
        }
        if self.pending_side_effects_dead_letter_after_secs == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsDeadLetterRetention);
        }
        if self.pending_side_effects_cleanup_interval_secs == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsCleanupInterval);
        }
        if u32::try_from(self.expiry_batch_size).is_err() {
            return Err(EntityConfigError::ExpiryBatchSizeOutOfRange);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EntityConfig, EntityConfigError};
    use pretty_assertions::assert_eq;

    fn valid_config() -> EntityConfig {
        EntityConfig {
            batch_concurrency: None,
            batch_chunk_concurrency: None,
            batch_chunk_write_concurrency: None,
            batch_chunk_size: 512,
            batch_large_threshold: 512,
            batch_prepare_blocking_threshold: 128,
            batch_async_side_effect_threshold: None,
            expiry_batch_size: 1_000,
            pending_side_effects_poll_secs: 1,
            pending_side_effects_poll_max_secs: 30,
            pending_side_effects_lease_secs: 30,
            pending_side_effects_batch_size: 32,
            pending_side_effects_max_attempts: 5,
            pending_side_effects_base_backoff_secs: 2,
            pending_side_effects_dead_letter_after_secs: 604_800,
            pending_side_effects_cleanup_interval_secs: 300,
        }
    }

    #[test]
    fn valid_entity_configuration_is_accepted() {
        assert_eq!(valid_config().validate(), Ok(()));
    }

    #[test]
    fn owner_defaults_keep_batch_and_expiry_limits_stable() {
        let config = EntityConfig::default();
        assert_eq!(config.batch_chunk_size, 512);
        assert_eq!(config.expiry_batch_size, 1_000);
    }

    #[test]
    fn zero_optional_concurrency_is_rejected_by_the_owner() {
        let mut config = valid_config();
        config.batch_chunk_write_concurrency = Some(0);
        assert_eq!(config.validate(), Err(EntityConfigError::ZeroBatchChunkWriteConcurrency));
    }

    #[test]
    fn zero_required_limit_is_rejected_by_the_owner() {
        let mut config = valid_config();
        config.expiry_batch_size = 0;
        assert_eq!(config.validate(), Err(EntityConfigError::ZeroExpiryBatchSize));
    }

    #[test]
    fn zero_dead_letter_retention_and_cadence_are_rejected() {
        let mut config = valid_config();
        config.pending_side_effects_dead_letter_after_secs = 0;
        assert_eq!(config.validate(), Err(EntityConfigError::ZeroPendingSideEffectsDeadLetterRetention));
        let mut config = valid_config();
        config.pending_side_effects_cleanup_interval_secs = 0;
        assert_eq!(config.validate(), Err(EntityConfigError::ZeroPendingSideEffectsCleanupInterval));
    }

    #[test]
    fn dead_letter_defaults_keep_evidence_for_a_week_and_reap_every_five_minutes() {
        let config = EntityConfig::default();
        assert_eq!(config.pending_side_effects_dead_letter_after_secs, 604_800);
        assert_eq!(config.pending_side_effects_cleanup_interval_secs, 300);
    }
}
