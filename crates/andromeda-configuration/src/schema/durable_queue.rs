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
use strum::Display;
use thiserror::Error;
use toml_comment::TomlComment;

/// Which durable queue a table configures, named by its key path.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum QueueName {
    /// `entity.side_effects.queue`.
    #[strum(serialize = "entity.side_effects.queue")]
    EntitySideEffects,
    /// `temporal.history.queue`.
    #[strum(serialize = "temporal.history.queue")]
    TemporalHistory,
    /// `notifications.delivery.queue`.
    #[strum(serialize = "notifications.delivery.queue")]
    NotificationDelivery,
}

/// What a queue table holds that its worker cannot run with.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum QueueViolation {
    /// The worker would poll without pause.
    #[error("poll_interval must be greater than zero")]
    ZeroPollInterval,
    /// The idle backoff would end below where it starts.
    #[error("max_poll_interval must be at least poll_interval")]
    PollCeilingBelowFloor,
    /// A claim would lapse the moment it was taken.
    #[error("lease_timeout must be greater than zero")]
    ZeroLeaseTimeout,
    /// A failed job would be retried at once.
    #[error("retry_interval must be greater than zero")]
    ZeroRetryInterval,
    /// The retry delay would be capped below its first step.
    #[error("max_retry_interval must be at least retry_interval")]
    RetryCeilingBelowFloor,
}

/// A queue table refused, with the queue it configures.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("{queue}.{violation}")]
pub struct DurableQueueError {
    /// The queue whose table was refused.
    pub queue: QueueName,
    /// What the table holds that the worker cannot run with.
    pub violation: QueueViolation,
}

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

impl DurableQueue {
    /// Check the table as the configuration of `queue`.
    ///
    /// # Errors
    ///
    /// Returns the [`DurableQueueError`] naming `queue` and the first value
    /// its worker cannot run with.
    pub const fn validate(&self, queue: QueueName) -> Result<(), DurableQueueError> {
        match self.violation() {
            Some(violation) => Err(DurableQueueError { queue, violation }),
            None => Ok(()),
        }
    }

    /// The first value the worker cannot run with, if any.
    const fn violation(&self) -> Option<QueueViolation> {
        if self.poll_interval.is_zero() {
            Some(QueueViolation::ZeroPollInterval)
        } else if self.max_poll_interval.duration().as_nanos() < self.poll_interval.duration().as_nanos() {
            Some(QueueViolation::PollCeilingBelowFloor)
        } else if self.lease_timeout.is_zero() {
            Some(QueueViolation::ZeroLeaseTimeout)
        } else if self.retry_interval.is_zero() {
            Some(QueueViolation::ZeroRetryInterval)
        } else if self.max_retry_interval.duration().as_nanos() < self.retry_interval.duration().as_nanos() {
            Some(QueueViolation::RetryCeilingBelowFloor)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::durable_queue::{DurableQueue, DurableQueueError, QueueName, QueueViolation},
    };
    use pretty_assertions::assert_eq;

    fn refused(queue: DurableQueue) -> Result<(), DurableQueueError> {
        queue.validate(QueueName::TemporalHistory)
    }

    #[test]
    fn the_default_queue_is_valid() {
        assert_eq!(refused(DurableQueue::default()), Ok(()));
    }

    #[test]
    fn a_poll_ceiling_below_its_floor_is_refused_and_an_equal_one_accepted() {
        let mut queue = DurableQueue {
            poll_interval: HumanDuration::from_secs(5),
            max_poll_interval: HumanDuration::from_millis(4_999),
            ..DurableQueue::default()
        };
        assert_eq!(
            refused(queue),
            Err(DurableQueueError {
                queue: QueueName::TemporalHistory,
                violation: QueueViolation::PollCeilingBelowFloor,
            })
        );
        queue.max_poll_interval = HumanDuration::from_secs(5);
        assert_eq!(refused(queue), Ok(()));
    }

    #[test]
    fn a_retry_ceiling_below_its_first_step_is_refused() {
        let queue = DurableQueue {
            retry_interval: HumanDuration::from_secs(10),
            max_retry_interval: HumanDuration::from_secs(9),
            ..DurableQueue::default()
        };
        assert_eq!(refused(queue).map_err(|error| error.violation), Err(QueueViolation::RetryCeilingBelowFloor));
    }

    #[test]
    fn zero_durations_are_refused() {
        for (queue, violation) in [
            (
                DurableQueue {
                    poll_interval: HumanDuration::from_secs(0),
                    ..DurableQueue::default()
                },
                QueueViolation::ZeroPollInterval,
            ),
            (
                DurableQueue {
                    lease_timeout: HumanDuration::from_secs(0),
                    ..DurableQueue::default()
                },
                QueueViolation::ZeroLeaseTimeout,
            ),
            (
                DurableQueue {
                    retry_interval: HumanDuration::from_secs(0),
                    ..DurableQueue::default()
                },
                QueueViolation::ZeroRetryInterval,
            ),
        ] {
            assert_eq!(refused(queue).map_err(|error| error.violation), Err(violation));
        }
    }

    #[test]
    fn the_error_names_the_key_path() {
        let error = DurableQueueError {
            queue: QueueName::NotificationDelivery,
            violation: QueueViolation::ZeroLeaseTimeout,
        };
        assert_eq!(error.to_string(), "notifications.delivery.queue.lease_timeout must be greater than zero");
    }

    #[test]
    fn a_key_outside_the_queue_table_is_refused() {
        let refused = toml::from_str::<DurableQueue>("dead_letter_after = \"1day\"\n");
        assert!(refused.is_err_and(|error| error.to_string().contains("unknown field `dead_letter_after`")));
    }
}
