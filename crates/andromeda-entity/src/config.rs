//! Entity execution and background-operation configuration.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, time::Duration};
use thiserror::Error;
use toml_comment::TomlComment;

/// How much longer than the connection acquire deadline a side-effect lease
/// must run.
///
/// A job's lease has to last from its claim to the first statement of its
/// replay, which removes the job and locks it for the rest of the replay. That
/// wait is chiefly acquiring the replay transaction's connection, which the
/// acquire deadline bounds; the margin covers the rest of it -- decoding the
/// claimed job, matching the notifications it still owes, and the settling
/// statement itself -- before another worker could reclaim the job.
pub const PENDING_SIDE_EFFECTS_LEASE_MARGIN: Duration = Duration::from_secs(5);

/// The default most JSON values one written entity may hold outside its
/// geometries and JSON literals: room for a temporal write that carries an
/// entity's history in one request, such as a smart meter's 5,000 readings of
/// eight attributes.
const DEFAULT_MAX_ENTITY_JSON_VALUES: NonZeroUsize = NonZeroUsize::MIN.saturating_add(999_999);

/// The default most positions one geometry may hold.
const DEFAULT_MAX_GEOMETRY_POSITIONS: NonZeroUsize = NonZeroUsize::MIN.saturating_add(99_999);

/// Invalid entity execution configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EntityConfigError {
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
    #[error("entity.pending_side_effects_poll must be greater than zero")]
    ZeroPendingSideEffectsPollCadence,
    /// The idle side-effect polling ceiling was configured below the polling floor.
    #[error("entity.pending_side_effects_poll_max must be at least entity.pending_side_effects_poll")]
    PendingSideEffectsPollCeilingBelowFloor,
    /// Pending side-effect lease duration was configured as zero.
    #[error("entity.pending_side_effects_lease must be greater than zero")]
    ZeroPendingSideEffectsLeaseDuration,
    /// Pending side-effect batch size was configured as zero.
    #[error("entity.pending_side_effects_batch_size must be at least 1")]
    ZeroPendingSideEffectsBatchSize,
    /// Pending side-effect attempt limit was configured as zero.
    #[error("entity.pending_side_effects_max_attempts must be at least 1")]
    ZeroPendingSideEffectsMaxAttempts,
    /// Pending side-effect backoff was configured as zero.
    #[error("entity.pending_side_effects_base_backoff must be greater than zero")]
    ZeroPendingSideEffectsBaseBackoff,
    /// Dead-lettered side-effect retention was configured as zero.
    #[error("entity.pending_side_effects_dead_letter_after must be greater than zero")]
    ZeroPendingSideEffectsDeadLetterRetention,
    /// Dead-lettered side-effect reap cadence was configured as zero.
    #[error("entity.pending_side_effects_cleanup_interval must be greater than zero")]
    ZeroPendingSideEffectsCleanupInterval,
    /// The side-effect lease does not outlast the connection acquire deadline
    /// by [`PENDING_SIDE_EFFECTS_LEASE_MARGIN`], so a job's lease could lapse
    /// before its replay locks it and another worker replay it instead.
    #[error(
        "entity.pending_side_effects_lease ({lease_seconds}s) must exceed database.acquire_timeout ({acquire_seconds}s) by at least {margin_seconds}s, the wait between claiming a job and its replay locking it"
    )]
    PendingSideEffectsLeaseWithoutMarginOverAcquire {
        /// The configured side-effect lease, in whole seconds.
        lease_seconds: u64,
        /// The configured connection acquire deadline, in whole seconds.
        acquire_seconds: u64,
        /// The margin the lease must leave over the acquire deadline, in seconds.
        margin_seconds: u64,
    },
}

/// Entity batching, expiry, and durable side-effect configuration.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct EntityConfig {
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
    /// idle backoff, in human-readable form (`"1s"`).
    ///
    /// After a commit's edge wake the drain runs promptly; while idle it backs
    /// off from here toward `pending_side_effects_poll_max`.
    #[toml_comment(inline)]
    pub pending_side_effects_poll: HumanDuration,
    /// Ceiling the side-effect drain's idle backoff reaches while no commit wakes
    /// it, in human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub pending_side_effects_poll_max: HumanDuration,
    /// Lease duration for a claimed side-effect job, in human-readable form
    /// (`"30s"`).
    ///
    /// It has to cover only the wait between a job's claim and the start of
    /// its replay, which locks the job for as long as the replay runs, and is
    /// refused unless it exceeds `database.acquire_timeout` by
    /// [`PENDING_SIDE_EFFECTS_LEASE_MARGIN`].
    #[toml_comment(inline)]
    pub pending_side_effects_lease: HumanDuration,
    /// Maximum jobs claimed per poll.
    pub pending_side_effects_batch_size: usize,
    /// Attempts before a side-effect job is dead-lettered.
    pub pending_side_effects_max_attempts: u16,
    /// Base retry backoff for side-effect jobs, in human-readable form (`"2s"`).
    #[toml_comment(inline)]
    pub pending_side_effects_base_backoff: HumanDuration,
    /// How long a side-effect job is kept once it is dead-lettered, in
    /// human-readable form (`"7days"`).
    ///
    /// A dead job is evidence of a batch whose deferred side effects never
    /// landed, so it is kept for a week by default after it gives up before the
    /// reap removes it.
    #[toml_comment(inline)]
    pub pending_side_effects_dead_letter_after: HumanDuration,
    /// Cadence of the dead-lettered side-effect reap, in human-readable form
    /// (`"5m"`).
    #[toml_comment(inline)]
    pub pending_side_effects_cleanup_interval: HumanDuration,
    /// Maximum number of JSON values one written entity may hold outside its
    /// geometries and JSON literals, counted on the document the write expands
    /// to; an entity over it is answered 413. Every replica of a deployment
    /// runs the same value.
    #[toml_comment(inline)]
    pub max_entity_json_values: NonZeroUsize,
    /// Maximum number of positions one `GeoProperty` geometry may hold, in a
    /// written entity and in a query, subscription or registration geometry.
    /// A typed entity's geometry is counted before it is expanded. A written
    /// geometry over it is answered 413, a query's 403
    /// `TooComplexQuery`. Every replica of a deployment runs the same value.
    #[toml_comment(inline)]
    pub max_geometry_positions: NonZeroUsize,
}

impl Default for EntityConfig {
    fn default() -> EntityConfig {
        EntityConfig {
            batch_chunk_concurrency: None,
            batch_chunk_write_concurrency: None,
            batch_chunk_size: 512,
            batch_large_threshold: 512,
            batch_prepare_blocking_threshold: 128,
            batch_async_side_effect_threshold: None,
            expiry_batch_size: 1_000,
            pending_side_effects_poll: HumanDuration::from_secs(1),
            pending_side_effects_poll_max: HumanDuration::from_secs(30),
            pending_side_effects_lease: HumanDuration::from_secs(30),
            pending_side_effects_batch_size: 32,
            pending_side_effects_max_attempts: 5,
            pending_side_effects_base_backoff: HumanDuration::from_secs(2),
            pending_side_effects_dead_letter_after: HumanDuration::from_secs(604_800),
            pending_side_effects_cleanup_interval: HumanDuration::from_secs(300),
            max_entity_json_values: DEFAULT_MAX_ENTITY_JSON_VALUES,
            max_geometry_positions: DEFAULT_MAX_GEOMETRY_POSITIONS,
        }
    }
}

impl EntityConfig {
    /// The budget a written entity payload is admitted under.
    #[must_use]
    pub const fn payload_budget(&self) -> EntityPayloadBudget {
        EntityPayloadBudget {
            json_values: JsonValueLimit::new(self.max_entity_json_values),
            geometry_positions: GeometryPositionLimit::new(self.max_geometry_positions),
        }
    }

    /// The limits a geometry standing on its own is admitted under: the
    /// reference geometry of a query or a subscription, or the geometry of a
    /// registration.
    #[must_use]
    pub const fn geometry_census_limits(&self) -> GeometryCensusLimits {
        self.payload_budget().geometry_census_limits()
    }

    /// Validate the entity policy and execution limits.
    ///
    /// # Errors
    ///
    /// Reports the matching [`EntityConfigError`] variant for the first limit
    /// that is zero, and [`EntityConfigError::ExpiryBatchSizeOutOfRange`] when
    /// the expiry batch size does not fit the unsigned 32-bit width the expiry
    /// source port accepts.
    pub fn validate(&self) -> Result<(), EntityConfigError> {
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
        if self.pending_side_effects_poll.is_zero() {
            return Err(EntityConfigError::ZeroPendingSideEffectsPollCadence);
        }
        if self.pending_side_effects_poll_max.duration().as_secs() < self.pending_side_effects_poll.duration().as_secs() {
            return Err(EntityConfigError::PendingSideEffectsPollCeilingBelowFloor);
        }
        if self.pending_side_effects_lease.is_zero() {
            return Err(EntityConfigError::ZeroPendingSideEffectsLeaseDuration);
        }
        if self.pending_side_effects_batch_size == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsBatchSize);
        }
        if self.pending_side_effects_max_attempts == 0 {
            return Err(EntityConfigError::ZeroPendingSideEffectsMaxAttempts);
        }
        if self.pending_side_effects_base_backoff.is_zero() {
            return Err(EntityConfigError::ZeroPendingSideEffectsBaseBackoff);
        }
        if self.pending_side_effects_dead_letter_after.is_zero() {
            return Err(EntityConfigError::ZeroPendingSideEffectsDeadLetterRetention);
        }
        if self.pending_side_effects_cleanup_interval.is_zero() {
            return Err(EntityConfigError::ZeroPendingSideEffectsCleanupInterval);
        }
        if u32::try_from(self.expiry_batch_size).is_err() {
            return Err(EntityConfigError::ExpiryBatchSizeOutOfRange);
        }
        Ok(())
    }

    /// Check that the side-effect lease outlasts the wait between a job's
    /// claim and its replay locking it, when acquiring a connection may take up
    /// to `acquire_timeout`.
    ///
    /// # Errors
    ///
    /// Returns
    /// [`EntityConfigError::PendingSideEffectsLeaseWithoutMarginOverAcquire`]
    /// when the lease is shorter than `acquire_timeout` plus
    /// [`PENDING_SIDE_EFFECTS_LEASE_MARGIN`].
    pub fn validate_side_effect_lease_over(&self, acquire_timeout: Duration) -> Result<(), EntityConfigError> {
        let lease = self.pending_side_effects_lease.duration();
        if lease < acquire_timeout.saturating_add(PENDING_SIDE_EFFECTS_LEASE_MARGIN) {
            return Err(EntityConfigError::PendingSideEffectsLeaseWithoutMarginOverAcquire {
                lease_seconds: lease.as_secs(),
                acquire_seconds: acquire_timeout.as_secs(),
                margin_seconds: PENDING_SIDE_EFFECTS_LEASE_MARGIN.as_secs(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{EntityConfig, EntityConfigError};
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroUsize, time::Duration};

    fn valid_config() -> EntityConfig {
        EntityConfig {
            batch_chunk_concurrency: None,
            batch_chunk_write_concurrency: None,
            batch_chunk_size: 512,
            batch_large_threshold: 512,
            batch_prepare_blocking_threshold: 128,
            batch_async_side_effect_threshold: None,
            expiry_batch_size: 1_000,
            pending_side_effects_poll: HumanDuration::from_secs(1),
            pending_side_effects_poll_max: HumanDuration::from_secs(30),
            pending_side_effects_lease: HumanDuration::from_secs(30),
            pending_side_effects_batch_size: 32,
            pending_side_effects_max_attempts: 5,
            pending_side_effects_base_backoff: HumanDuration::from_secs(2),
            pending_side_effects_dead_letter_after: HumanDuration::from_secs(604_800),
            pending_side_effects_cleanup_interval: HumanDuration::from_secs(300),
            max_entity_json_values: NonZeroUsize::MIN.saturating_add(99_999),
            max_geometry_positions: NonZeroUsize::MIN.saturating_add(99_999),
        }
    }

    #[test]
    fn the_default_budgets_admit_one_hundred_thousand_values_and_positions() {
        let budget = EntityConfig::default().payload_budget();
        assert_eq!(budget.json_values.get(), 1_000_000);
        assert_eq!(budget.geometry_positions.get(), 100_000);
    }

    #[test]
    fn the_budgets_parse_from_toml_and_a_zero_budget_does_not() {
        let rendered = toml::to_string(&EntityConfig::default()).expect("the defaults render");
        let parsed: EntityConfig = toml::from_str(&rendered.replace("max_geometry_positions = 100000", "max_geometry_positions = 250000")).expect("parses");
        assert_eq!(parsed.max_geometry_positions.get(), 250_000);
        assert!(toml::from_str::<EntityConfig>(&rendered.replace("max_entity_json_values = 1000000", "max_entity_json_values = 0")).is_err());
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
        config.pending_side_effects_dead_letter_after = HumanDuration::from_secs(0);
        assert_eq!(config.validate(), Err(EntityConfigError::ZeroPendingSideEffectsDeadLetterRetention));
        let mut config = valid_config();
        config.pending_side_effects_cleanup_interval = HumanDuration::from_secs(0);
        assert_eq!(config.validate(), Err(EntityConfigError::ZeroPendingSideEffectsCleanupInterval));
    }

    #[test]
    fn dead_letter_defaults_keep_evidence_for_a_week_and_reap_every_five_minutes() {
        let config = EntityConfig::default();
        assert_eq!(config.pending_side_effects_dead_letter_after.duration().as_secs(), 604_800);
        assert_eq!(config.pending_side_effects_cleanup_interval.duration().as_secs(), 300);
    }

    /// A lease that could lapse while its replay still waits for a connection
    /// lets another worker reclaim and replay the job instead.
    #[test]
    fn a_side_effect_lease_without_the_margin_over_the_acquire_deadline_is_rejected() {
        let mut config = valid_config();
        config.pending_side_effects_lease = HumanDuration::from_secs(9);
        assert_eq!(
            config.validate_side_effect_lease_over(Duration::from_secs(5)),
            Err(EntityConfigError::PendingSideEffectsLeaseWithoutMarginOverAcquire {
                lease_seconds: 9,
                acquire_seconds: 5,
                margin_seconds: 5,
            })
        );
    }

    #[test]
    fn a_side_effect_lease_covering_the_acquire_deadline_and_the_margin_is_accepted() {
        let mut config = valid_config();
        config.pending_side_effects_lease = HumanDuration::from_secs(10);
        assert_eq!(config.validate_side_effect_lease_over(Duration::from_secs(5)), Ok(()));
    }

    /// The shipped side-effect lease covers the shipped acquire deadline.
    #[test]
    fn the_default_side_effect_lease_covers_the_default_acquire_deadline() {
        assert_eq!(EntityConfig::default().validate_side_effect_lease_over(Duration::from_secs(5)), Ok(()));
    }
}
