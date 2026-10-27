//! The `[entity.side_effects]` tables: the durable queue of a committed
//! batch's deferred side effects.

use crate::{
    human_duration::HumanDuration,
    schema::durable_queue::{DurableQueue, DurableQueueError, QueueName},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use toml_comment::TomlComment;

/// How much longer than the connection acquire deadline a side-effect lease
/// must run.
///
/// A job's lease has to last from its claim to the first statement of its
/// replay, which removes the job and locks it for the rest of the replay. That
/// wait is chiefly acquiring the replay transaction's connection, which the
/// acquire deadline bounds; the margin covers the rest of it, decoding the
/// claimed job, matching the notifications it still owes, and the settling
/// statement itself, before another worker could reclaim the job.
pub const SIDE_EFFECT_LEASE_MARGIN: Duration = Duration::from_secs(5);

/// An unusable `[entity.side_effects]` table.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SideEffectsConfigError {
    /// The queue table is unusable.
    #[error(transparent)]
    Queue(#[from] DurableQueueError),
    /// Dead-lettered jobs would be removed the moment they give up.
    #[error("entity.side_effects.retention.dead_letter_after must be greater than zero")]
    ZeroDeadLetterAfter,
    /// The dead-letter removal would run without pause.
    #[error("entity.side_effects.retention.cleanup_interval must be greater than zero")]
    ZeroCleanupInterval,
    /// The lease does not outlast the connection acquire deadline by
    /// [`SIDE_EFFECT_LEASE_MARGIN`], so a job's lease could lapse before its
    /// replay locks it and another worker replay it instead.
    #[error(
        "entity.side_effects.queue.lease_timeout ({lease_seconds}s) must exceed database.acquire_timeout ({acquire_seconds}s) by at least {margin_seconds}s, the wait between claiming a job and its replay locking it"
    )]
    LeaseWithoutMarginOverAcquire {
        /// The configured lease, in whole seconds.
        lease_seconds: u64,
        /// The configured connection acquire deadline, in whole seconds.
        acquire_seconds: u64,
        /// The margin the lease must leave over the acquire deadline, in seconds.
        margin_seconds: u64,
    },
}

/// How long a dead-lettered side-effect job is kept.
///
/// A job that succeeds is removed by its own replay; only one that ran out of
/// attempts stays, as evidence of a batch whose side effects never landed.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SideEffectRetentionConfig {
    /// How long a dead-lettered job is kept, in human-readable form
    /// (`"7days"`).
    #[toml_comment(inline)]
    pub dead_letter_after: HumanDuration,
    /// How often expired dead-lettered jobs are removed, in human-readable
    /// form (`"5m"`).
    #[toml_comment(inline)]
    pub cleanup_interval: HumanDuration,
}

impl Default for SideEffectRetentionConfig {
    fn default() -> SideEffectRetentionConfig {
        SideEffectRetentionConfig {
            dead_letter_after: HumanDuration::from_days(7),
            cleanup_interval: HumanDuration::from_secs(300),
        }
    }
}

/// The durable queue of a committed batch's deferred side effects.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SideEffectsConfig {
    /// How the queue is polled, claimed and retried. A job is retried on a
    /// delay that doubles from `retry_interval`.
    pub queue: DurableQueue,
    /// How long a dead-lettered job is kept.
    pub retention: SideEffectRetentionConfig,
}

impl SideEffectsConfig {
    /// Validate the queue and the retention.
    ///
    /// # Errors
    ///
    /// Returns the [`SideEffectsConfigError`] of the first unusable value.
    pub const fn validate(&self) -> Result<(), SideEffectsConfigError> {
        if let Err(error) = self.queue.validate(QueueName::EntitySideEffects) {
            return Err(SideEffectsConfigError::Queue(error));
        }
        if self.retention.dead_letter_after.is_zero() {
            return Err(SideEffectsConfigError::ZeroDeadLetterAfter);
        }
        if self.retention.cleanup_interval.is_zero() {
            return Err(SideEffectsConfigError::ZeroCleanupInterval);
        }
        Ok(())
    }

    /// Check that the lease outlasts the wait between a job's claim and its
    /// replay locking it, when acquiring a connection may take up to
    /// `acquire_timeout`.
    ///
    /// # Errors
    ///
    /// Returns [`SideEffectsConfigError::LeaseWithoutMarginOverAcquire`] when
    /// the lease is shorter than `acquire_timeout` plus
    /// [`SIDE_EFFECT_LEASE_MARGIN`].
    pub const fn validate_lease_over(&self, acquire_timeout: Duration) -> Result<(), SideEffectsConfigError> {
        let lease = self.queue.lease_timeout.duration();
        if lease.as_nanos() < acquire_timeout.saturating_add(SIDE_EFFECT_LEASE_MARGIN).as_nanos() {
            return Err(SideEffectsConfigError::LeaseWithoutMarginOverAcquire {
                lease_seconds: lease.as_secs(),
                acquire_seconds: acquire_timeout.as_secs(),
                margin_seconds: SIDE_EFFECT_LEASE_MARGIN.as_secs(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::entity::side_effects::{SideEffectRetentionConfig, SideEffectsConfig, SideEffectsConfigError},
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_defaults_are_valid_and_outlast_a_five_second_acquire() {
        let config = SideEffectsConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(config.validate_lease_over(Duration::from_secs(5)), Ok(()));
        assert_eq!(config.queue.batch_size.get(), 32);
    }

    #[test]
    fn zero_retention_values_are_refused() {
        let zero = HumanDuration::from_secs(0);
        let no_dead_letters = SideEffectsConfig {
            retention: SideEffectRetentionConfig {
                dead_letter_after: zero,
                ..SideEffectRetentionConfig::default()
            },
            ..SideEffectsConfig::default()
        };
        assert_eq!(no_dead_letters.validate(), Err(SideEffectsConfigError::ZeroDeadLetterAfter));
        let no_cleanup = SideEffectsConfig {
            retention: SideEffectRetentionConfig {
                cleanup_interval: zero,
                ..SideEffectRetentionConfig::default()
            },
            ..SideEffectsConfig::default()
        };
        assert_eq!(no_cleanup.validate(), Err(SideEffectsConfigError::ZeroCleanupInterval));
    }

    #[test]
    fn a_lease_inside_the_acquire_margin_is_refused_and_one_at_it_accepted() {
        let mut config = SideEffectsConfig::default();
        config.queue.lease_timeout = HumanDuration::from_secs(9);
        assert_eq!(
            config.validate_lease_over(Duration::from_secs(5)),
            Err(SideEffectsConfigError::LeaseWithoutMarginOverAcquire {
                lease_seconds: 9,
                acquire_seconds: 5,
                margin_seconds: 5,
            })
        );
        config.queue.lease_timeout = HumanDuration::from_secs(10);
        assert_eq!(config.validate_lease_over(Duration::from_secs(5)), Ok(()));
    }
}
