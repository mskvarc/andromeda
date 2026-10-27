//! The `[temporal.history]` tables: what is recorded in a Temporal Evolution
//! and how the queue that records it is drained.

use crate::{
    human_duration::HumanDuration,
    schema::{durable_queue::DurableQueue, temporal::core_write_history::CoreWriteHistory},
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use toml_comment::TomlComment;

/// What is recorded in a Temporal Evolution, and how the queue that records it
/// is drained.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemporalHistoryConfig {
    /// Whether Core API writes add to the Temporal Evolution of the Entities
    /// they change: `recorded` or `not-recorded`.
    ///
    /// Under `recorded` a Core write records the attribute instances it
    /// modified and nothing for a restatement of what the Entity holds. Under
    /// `not-recorded` only the Temporal API writes history, for a producer
    /// that publishes its history there alone. Core writes change current
    /// state, and Core queries see them, either way.
    #[toml_comment(inline)]
    pub core_writes: CoreWriteHistory,
    /// The queue of committed history intents. One pass applies its batch in
    /// one transaction, so a wider batch spreads each statement's cost over
    /// more intents. A failed intent is retried on a delay that grows
    /// linearly from `retry_interval`, and one that keeps failing is reported
    /// stuck after `max_attempts`, never discarded.
    pub queue: DurableQueue,
}

impl Default for TemporalHistoryConfig {
    fn default() -> TemporalHistoryConfig {
        TemporalHistoryConfig {
            core_writes: CoreWriteHistory::Recorded,
            queue: DurableQueue {
                batch_size: NonZeroU32::MIN.saturating_add(255),
                max_attempts: NonZeroU32::MIN.saturating_add(9),
                retry_interval: HumanDuration::from_secs(2),
                ..DurableQueue::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::temporal::{core_write_history::CoreWriteHistory, history::TemporalHistoryConfig};
    use pretty_assertions::assert_eq;

    /// A respond-async write of 500 entities drains in two batches.
    #[test]
    fn the_defaults_record_core_writes_and_drain_256_intents_a_pass() {
        let config = TemporalHistoryConfig::default();
        assert_eq!(config.core_writes, CoreWriteHistory::Recorded);
        assert_eq!(config.queue.batch_size.get(), 256);
        assert_eq!(config.queue.max_attempts.get(), 10);
    }
}
