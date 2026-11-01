//! A delivery that fails at every attempt, retried at intervals that double from the stack's retry interval, and the
//! attempts it receives after its tenant was seen dormant.
//!
//! The dormant broker allows ten attempts, so a chain keeps retrying for about 42 minutes, longer than any eviction
//! takes. An attempt that arrives after the tenant was seen dormant was sent by a runtime only the wake loop can have
//! loaded: nothing else touches the tenant again.

use crate::notification_sink::{NotificationSink, SinkError};
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// The first retry interval of a failed delivery (`notifications.delivery.queue.retry_interval`, the 5 s default).
pub const RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// What a due retry of a dormant tenant waits for beyond its due instant: the 3 s the stack lets pass before an
/// evicted tenant is loaded again, the 5 s sweep, the load itself, the drain's first pass and one round trip, with a
/// wide margin.
pub const WAKE_MARGIN: Duration = Duration::from_secs(30);

/// How long after the tenant was seen dormant its attempts are counted: a delivery the evicted runtime had already
/// sent lands within it, while no wake can load the tenant before 3 s have passed.
pub const IN_FLIGHT_GRACE: Duration = Duration::from_secs(1);

/// The attempts a failing chain received around its tenant's dormancy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DormantAttempts {
    /// The attempts received once the tenant was seen dormant and every in-flight attempt had landed.
    pub before: usize,
    /// The attempts received in all once a later one arrived or the bound passed.
    pub total: usize,
}

/// A failing delivery chain whose first attempt arrived.
#[derive(Debug, Clone, Copy)]
pub struct RetryChain<'sink> {
    /// The sink receiving the attempts.
    sink: &'sink NotificationSink,
    /// The endpoint of the sink the attempts go to.
    endpoint: &'static str,
    /// When the first attempt arrived.
    first_attempt: Instant,
}

impl<'sink> RetryChain<'sink> {
    /// The chain whose attempts go to `endpoint` of `sink`, once its first attempt arrived.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] when no attempt arrives within the delivery deadline.
    pub async fn started(sink: &'sink NotificationSink, endpoint: &'static str) -> Result<RetryChain<'sink>, SinkError> {
        sink.wait_for(endpoint, 1).await?;
        Ok(RetryChain {
            sink,
            endpoint,
            first_attempt: Instant::now(),
        })
    }

    /// The attempts before and after the tenant was seen dormant at `dormant_at`.
    ///
    /// The wait is bounded by the chain itself. Each interval doubles the one before, so the interval running at
    /// `dormant_at`, which began at most `dormant_at - first_attempt` after the first attempt, ends at most that long
    /// plus [`RETRY_INTERVAL`] after it; a wake serves the retry within [`WAKE_MARGIN`].
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] when the sink cannot be read.
    pub async fn attempts_after(&self, dormant_at: Instant) -> Result<DormantAttempts, SinkError> {
        sleep(IN_FLIGHT_GRACE).await;
        let before = self.sink.received(self.endpoint).await?.len();
        let deadline = dormant_at + dormant_at.saturating_duration_since(self.first_attempt) + RETRY_INTERVAL + WAKE_MARGIN;
        loop {
            let total = self.sink.received(self.endpoint).await?.len();
            if total > before || Instant::now() >= deadline {
                return Ok(DormantAttempts { before, total });
            }
            sleep(Duration::from_millis(250)).await;
        }
    }
}
