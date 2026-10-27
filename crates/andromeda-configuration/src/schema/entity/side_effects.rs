//! The `[entity.side_effects]` tables: the durable queue of a committed
//! batch's deferred side effects.

use crate::{human_duration::HumanDuration, schema::durable_queue::DurableQueue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How long a dead-lettered side-effect job is kept.
///
/// A job that succeeds is removed by its own replay; only one that ran out of
/// attempts stays, as evidence of a batch whose side effects never landed.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "How long a dead-lettered side-effect job is kept, and how often expired ones are removed.")]
#[serde(deny_unknown_fields)]
pub struct SideEffectRetentionConfig {
    /// How long a dead-lettered job is kept, in human-readable form
    /// (`"7days"`).
    #[schemars(
        description = "How long a job that ran out of attempts is kept, as a record of a batch whose notifications were never raised. A job that succeeds is removed by its own replay."
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
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[schemars(
    description = "The durable queue of change events a `Prefer: respond-async` batch stores with its entities for a worker to turn into notifications."
)]
#[serde(deny_unknown_fields)]
pub struct SideEffectsConfig {
    /// How the queue is polled, claimed and retried. A job is retried on a
    /// delay that doubles from `retry_interval`.
    #[schemars(
        description = "How the queue is polled, claimed and retried. A failed job's retry delay doubles from `entity.side_effects.queue.retry_interval`."
    )]
    pub queue: DurableQueue,
    /// How long a dead-lettered job is kept.
    #[schemars(description = "How long a dead-lettered job is kept.")]
    pub retention: SideEffectRetentionConfig,
}

#[cfg(test)]
mod tests {
    use crate::schema::entity::side_effects::SideEffectsConfig;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_queue_claims_thirty_two_jobs_a_pass() {
        let config = SideEffectsConfig::default();
        assert_eq!(config.queue.batch_size.get(), 32);
    }
}
