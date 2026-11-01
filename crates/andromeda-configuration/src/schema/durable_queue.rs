//! The one table every durable work queue is configured with.
//!
//! The entity side-effect queue, the temporal history queue and the
//! notification delivery queue are drained the same way: a worker polls,
//! claims jobs under a lease, and retries a failed job on a growing delay.
//! They therefore share one table, so an operator tunes every queue with the
//! same keys, each meaning the same in every queue: the retry delay doubles
//! from `retry_interval` up to `max_retry_interval` in all three. How many
//! jobs a pass claims, and what happens to a job that keeps failing, differ
//! by queue, so those keys live in the table that owns the queue.

use crate::human_duration::HumanDuration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a durable queue is polled, claimed and retried.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(description = "How a durable work queue is polled, claimed and retried.")]
#[schemars(deny_unknown_fields)]
pub struct DurableQueue {
    /// How long the worker waits before polling again once a commit's wake
    /// has been handled, in human-readable form (`"1s"`). The floor of its
    /// idle backoff.
    #[schemars(
        description = "How soon an idle worker polls again once a commit's wake has been handled: the floor of its idle backoff. A commit in the same broker process wakes the worker at once."
    )]
    pub poll_interval: HumanDuration,
    /// The longest the idle backoff grows to while no commit wakes the worker,
    /// in human-readable form (`"30s"`). Also how soon a job committed on
    /// another replica is picked up.
    #[schemars(
        description = "The longest the idle backoff grows to while no commit wakes the worker. Also how soon a job committed by another broker process is picked up."
    )]
    pub max_poll_interval: HumanDuration,
    /// How long a claim keeps a job from every other worker, in human-readable
    /// form (`"30s"`). A worker that dies leaves its jobs unclaimable for the
    /// rest of the term.
    #[schemars(
        description = "How long a claim keeps a job from every other worker. A worker that dies leaves its jobs unclaimable for the rest of the term. `notifications.delivery.queue` reads it in whole seconds, rounded down."
    )]
    pub lease_timeout: HumanDuration,
    /// The delay before the first retry of a failed job, in human-readable
    /// form (`"2s"`). Each later retry waits twice as long as the one before,
    /// up to `max_retry_interval`.
    #[schemars(
        description = "The delay before a failed job's first retry. Each later retry waits twice as long as the one before, up to the longest retry delay."
    )]
    pub retry_interval: HumanDuration,
    /// The longest a retry delay grows to, in human-readable form (`"5m"`).
    #[schemars(description = "The longest a retry delay grows to.")]
    pub max_retry_interval: HumanDuration,
}

impl Default for DurableQueue {
    fn default() -> DurableQueue {
        DurableQueue {
            poll_interval: HumanDuration::from_secs(1),
            max_poll_interval: HumanDuration::from_secs(30),
            lease_timeout: HumanDuration::from_secs(30),
            retry_interval: HumanDuration::from_secs(2),
            max_retry_interval: HumanDuration::from_secs(300),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::durable_queue::DurableQueue;
    use pretty_assertions::assert_eq;
    use toml::de::Error as TomlError;

    /// How a job that keeps failing is handled differs by queue, so
    /// `dead_letter_after` lives in the table that owns the queue and the
    /// shared table does not read it.
    #[test]
    fn a_key_outside_the_queue_table_is_not_read() -> Result<(), TomlError> {
        let table: toml::Value = toml::from_str(concat!(
            "poll_interval = \"1s\"\nmax_poll_interval = \"30s\"\nlease_timeout = \"30s\"\n",
            "retry_interval = \"2s\"\nmax_retry_interval = \"5m\"\ndead_letter_after = \"1day\"\n",
        ))?;
        let mut ignored = Vec::new();
        let queue = serde_ignored::deserialize::<_, _, DurableQueue>(table, |path| ignored.push(path.to_string()))?;
        assert_eq!(queue, DurableQueue::default());
        assert_eq!(ignored, ["dead_letter_after"]);
        Ok(())
    }
}
