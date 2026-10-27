//! The one table every durable work queue is configured with.
//!
//! The entity side-effect queue, the temporal history queue and the
//! notification delivery queue are drained the same way: a worker polls,
//! claims a batch under a lease, and retries a failed job on a growing delay.
//! They therefore share one table, so an operator tunes every queue with the
//! same keys. How a queue's delay grows, and what happens to a job that runs
//! out of attempts, belong to the queue's own domain.

use crate::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use toml_comment::TomlComment;

/// How a durable queue is polled, claimed and retried.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DurableQueue {
    /// How long the worker waits before polling again once a commit's wake
    /// has been handled, in human-readable form (`"1s"`). The floor of its
    /// idle backoff.
    #[toml_comment(inline)]
    pub poll_interval: HumanDuration,
    /// The longest the idle backoff grows to while no commit wakes the worker,
    /// in human-readable form (`"30s"`). Also how soon a job committed on
    /// another replica is picked up.
    #[toml_comment(inline)]
    pub max_poll_interval: HumanDuration,
    /// How long a claim keeps a job from every other worker, in human-readable
    /// form (`"30s"`). A worker that dies leaves its jobs unclaimable for the
    /// rest of the term.
    #[toml_comment(inline)]
    pub lease_timeout: HumanDuration,
    /// Jobs one pass claims at most.
    #[toml_comment(inline)]
    pub batch_size: NonZeroU32,
    /// Attempts after which a job that keeps failing leaves the retry path:
    /// a side effect or a delivery is dead-lettered, a temporal history
    /// intent is reported stuck and stays claimable, since it is the only
    /// record of history the broker accepted.
    #[toml_comment(inline)]
    pub max_attempts: NonZeroU32,
    /// The delay before the first retry of a failed job, in human-readable
    /// form (`"2s"`). Later retries wait longer, as the queue's domain grows
    /// the delay.
    #[toml_comment(inline)]
    pub retry_interval: HumanDuration,
    /// The longest a retry delay grows to, in human-readable form (`"5m"`).
    #[toml_comment(inline)]
    pub max_retry_interval: HumanDuration,
}

impl Default for DurableQueue {
    fn default() -> DurableQueue {
        DurableQueue {
            poll_interval: HumanDuration::from_secs(1),
            max_poll_interval: HumanDuration::from_secs(30),
            lease_timeout: HumanDuration::from_secs(30),
            batch_size: NonZeroU32::MIN.saturating_add(31),
            max_attempts: NonZeroU32::MIN.saturating_add(4),
            retry_interval: HumanDuration::from_secs(2),
            max_retry_interval: HumanDuration::from_secs(300),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::durable_queue::DurableQueue;

    #[test]
    fn a_key_outside_the_queue_table_is_refused() {
        let refused = toml::from_str::<DurableQueue>(
            "dead_letter_after = \"1day\"
",
        );
        assert!(refused.is_err_and(|error| error.to_string().contains("unknown field `dead_letter_after`")));
    }
}
