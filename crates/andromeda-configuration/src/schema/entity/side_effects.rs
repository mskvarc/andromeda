//! The `[entity.side_effects]` tables: the durable queue of a committed
//! batch's deferred side effects.

use crate::{human_duration::HumanDuration, schema::durable_queue::DurableQueue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// How long a dead-lettered side-effect job is kept.
///
/// A job that succeeds is removed by its own replay, and one that fails is
/// retried for as long as it fails; only one whose stored payload cannot be
/// read back is dead-lettered, as evidence of a batch whose side effects never
/// landed.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "How long a dead-lettered side-effect job is kept, and how often expired ones are removed.")]
#[serde(deny_unknown_fields)]
pub struct SideEffectRetentionConfig {
    /// How long a dead-lettered job is kept, in human-readable form
    /// (`"7days"`).
    #[schemars(
        description = "How long a job whose stored changes cannot be read back is kept, as a record of a batch whose notifications were never raised. A job that succeeds is removed by its own replay, and one that fails is retried for as long as it fails."
    )]
    pub dead_letter_after: HumanDuration,
    /// How often expired dead-lettered jobs are removed, in human-readable
    /// form (`"5m"`).
    #[schemars(description = "How often dead-lettered jobs older than `entity.side_effects.retention.dead_letter_after` are removed.")]
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
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(
    description = "The durable queue of change events a `Prefer: respond-async` batch stores with its entities for a worker to turn into notifications."
)]
#[serde(deny_unknown_fields)]
pub struct SideEffectsConfig {
    /// Jobs one pass claims at most; the pass replays them one after another.
    #[schemars(description = "The most jobs one pass claims. The pass replays them one after another before the worker claims again.")]
    pub batch_size: NonZeroU32,
    /// Failed attempts from which a job is reported stuck on every further
    /// failure. It is never dropped: it holds the notifications of a write the
    /// broker already accepted.
    #[schemars(
        description = "Failed attempts from which a job that keeps failing is reported stuck, logged and counted on each further failure. It is never dropped, since it holds the notifications of a write the broker already accepted, and it keeps being retried, never waiting longer than `entity.side_effects.queue.max_retry_interval`."
    )]
    pub stuck_after_attempts: NonZeroU32,
    /// How the queue is polled, claimed and retried. A job is retried on a
    /// delay that doubles from `retry_interval` up to `max_retry_interval`,
    /// however often it fails.
    #[schemars(
        description = "How the queue is polled, claimed and retried. A failed job is retried on a delay that doubles from `entity.side_effects.queue.retry_interval` up to `entity.side_effects.queue.max_retry_interval`, however often it fails, and is never dropped."
    )]
    pub queue: DurableQueue,
    /// How long a dead-lettered job is kept.
    #[schemars(description = "How long a dead-lettered job is kept.")]
    pub retention: SideEffectRetentionConfig,
}

impl Default for SideEffectsConfig {
    fn default() -> SideEffectsConfig {
        SideEffectsConfig {
            batch_size: NonZeroU32::MIN.saturating_add(31),
            stuck_after_attempts: NonZeroU32::MIN.saturating_add(4),
            queue: DurableQueue::default(),
            retention: SideEffectRetentionConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::entity::side_effects::SideEffectsConfig;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_queue_claims_thirty_two_jobs_a_pass() {
        let config = SideEffectsConfig::default();
        assert_eq!(config.batch_size.get(), 32);
    }
}
