//! The `[temporal]` tables: reading, recording and storing Temporal
//! Evolutions.

pub mod core_write_history;
pub mod error;
pub mod history;
pub mod query;
pub mod storage;

use crate::schema::{
    durable_queue::QueueName,
    temporal::{error::TemporalConfigError, history::TemporalHistoryConfig, query::TemporalQueryConfig, storage::TemporalStorageConfig},
};
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// How Temporal Evolutions are read, recorded and stored.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemporalConfig {
    /// How much of a Temporal Evolution one read returns.
    pub query: TemporalQueryConfig,
    /// What is recorded, and how the recording queue is drained.
    pub history: TemporalHistoryConfig,
    /// How history is partitioned, converted and aged out.
    pub storage: TemporalStorageConfig,
}

impl TemporalConfig {
    /// Validate the query bounds, the history queue and the storage.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalConfigError`] of the first value refused.
    pub const fn validate(&self) -> Result<(), TemporalConfigError> {
        if let Err(error) = self.query.validate() {
            return Err(error);
        }
        if let Err(error) = self.history.queue.validate(QueueName::TemporalHistory) {
            return Err(TemporalConfigError::HistoryQueue(error));
        }
        self.storage.validate()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::{
            durable_queue::{DurableQueueError, QueueName, QueueViolation},
            temporal::{TemporalConfig, error::TemporalConfigError},
        },
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn the_defaults_are_valid() {
        assert_eq!(TemporalConfig::default().validate(), Ok(()));
    }

    #[test]
    fn an_unusable_history_queue_is_refused_by_its_path() {
        let mut config = TemporalConfig::default();
        config.history.queue.max_poll_interval = config.history.queue.poll_interval;
        config.history.queue.poll_interval = HumanDuration::from_secs(60);
        assert_eq!(
            config.validate(),
            Err(TemporalConfigError::HistoryQueue(DurableQueueError {
                queue: QueueName::TemporalHistory,
                violation: QueueViolation::PollCeilingBelowFloor,
            }))
        );
    }
}
