//! The `[temporal.history]` tables: what is recorded in a Temporal Evolution
//! and how the queue that records it is drained.

use crate::schema::{durable_queue::DurableQueue, temporal::core_write_history::CoreWriteHistory};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// What is recorded in a Temporal Evolution, and how the queue that records it
/// is drained.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "What is recorded in history, and how the queue of committed history writes is drained.")]
pub struct TemporalHistoryConfig {
    /// Whether Core API writes add to the Temporal Evolution of the Entities
    /// they change: `recorded` or `not-recorded`.
    ///
    /// Under `recorded` a Core write records the attribute instances it
    /// modified and nothing for a restatement of what the Entity holds. Under
    /// `not-recorded` only the Temporal API writes history, for a producer
    /// that publishes its history there alone. Core writes change current
    /// state, and Core queries see them, either way.
    #[schemars(
        description = "Whether Core API writes add to the history of the entities they change. With `\"not-recorded\"` only the Temporal API writes history, for a producer that publishes its history there alone; Core writes change current state, and Core queries see them, either way."
    )]
    pub core_writes: CoreWriteHistory,
    /// History intents one pass claims at most. The pass applies them in one
    /// transaction, so a wider batch spreads each statement's cost over more
    /// intents.
    #[schemars(
        description = "The most history writes one pass claims. The pass applies them in one transaction, so a larger batch spreads each statement's cost over more writes."
    )]
    pub batch_size: NonZeroU32,
    /// Failed attempts from which a history intent is reported stuck on
    /// every further failure. It is never discarded: it is history the broker
    /// already accepted.
    #[schemars(
        description = "Failed attempts from which a history write that keeps failing is reported stuck, logged and counted on each further failure. It is never discarded, since it is history the broker has already accepted, and it keeps being retried, never waiting longer than `temporal.history.queue.max_retry_interval`."
    )]
    pub stuck_after_attempts: NonZeroU32,
    /// The queue of committed history intents. A failed intent is retried on
    /// a delay that doubles from `retry_interval` up to `max_retry_interval`.
    #[schemars(
        description = "The queue of committed history writes, including those of `respond-async` batches. A failed write is retried on a delay that doubles from `temporal.history.queue.retry_interval` up to `temporal.history.queue.max_retry_interval`."
    )]
    pub queue: DurableQueue,
}

impl Default for TemporalHistoryConfig {
    fn default() -> TemporalHistoryConfig {
        TemporalHistoryConfig {
            core_writes: CoreWriteHistory::Recorded,
            batch_size: NonZeroU32::MIN.saturating_add(255),
            stuck_after_attempts: NonZeroU32::MIN.saturating_add(9),
            queue: DurableQueue::default(),
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
        assert_eq!(config.batch_size.get(), 256);
        assert_eq!(config.stuck_after_attempts.get(), 10);
    }
}
