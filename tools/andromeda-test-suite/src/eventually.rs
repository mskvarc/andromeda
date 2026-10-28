//! The one way the suite waits for the broker: poll until a condition holds or a deadline passes.
//!
//! Notifications, forwarded requests and records the broker writes after answering arrive on the broker's schedule,
//! while other tests keep it busy. A wait therefore polls with a bound, and the bound is generous: a passing wait
//! ends at the first poll that sees the condition, so only a failing one costs the whole bound.

use crate::config::SuiteConfig;
use std::{future::Future, time::Duration};
use tokio::time::{Instant, sleep};

/// The longest a positive wait waits when the stack sets no `ANDROMEDA_TEST_SUITE__DELIVERY_DEADLINE_SECONDS`.
///
/// A positive wait (a notification, a forwarded request, an MQTT message, a record the broker writes after answering)
/// ends at the first poll that sees its condition, so the deadline costs only a failing test. The ETSI retry budgets
/// (1 s, 5 s, 10 s) are shorter than the broker's delivery latency when several runs share one stack. Absence windows
/// and the real-time waits a case requires (a Sleep, an expiry, a throttling) are not deadlines and do not use it.
pub const DEFAULT_DELIVERY_DEADLINE: Duration = Duration::from_secs(60);

/// The deadline of every positive wait: the stack's `ANDROMEDA_TEST_SUITE__DELIVERY_DEADLINE_SECONDS`, or
/// [`DEFAULT_DELIVERY_DEADLINE`].
#[must_use]
pub fn delivery_deadline() -> Duration {
    SuiteConfig::get()
        .ok()
        .and_then(SuiteConfig::delivery_deadline)
        .unwrap_or(DEFAULT_DELIVERY_DEADLINE)
}

/// The pause between two polls.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The outcome of one poll.
#[derive(Debug)]
pub enum Poll<T> {
    /// The condition holds, with what the wait answers.
    Ready(T),
    /// The condition does not hold yet; the text says what was seen, for the failure message.
    Pending(String),
}

/// Why a wait ended without its condition.
#[derive(Debug, thiserror::Error)]
#[error("the condition did not hold within {bound:?}: {last}")]
pub struct TimedOut {
    /// The bound waited.
    pub bound: Duration,
    /// What the last poll saw.
    pub last: String,
}

/// Polls `poll` every [`POLL_INTERVAL`] until it is ready or `bound` has passed since the first poll.
///
/// # Errors
///
/// Returns [`TimedOut`] with what the last poll saw.
pub async fn eventually<T, F, Fut>(bound: Duration, mut poll: F) -> Result<T, TimedOut>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Poll<T>>,
{
    let deadline = Instant::now() + bound;
    loop {
        match poll().await {
            Poll::Ready(value) => return Ok(value),
            Poll::Pending(last) if Instant::now() >= deadline => return Err(TimedOut { bound, last }),
            Poll::Pending(_) => sleep(POLL_INTERVAL).await,
        }
    }
}

/// Repeats `attempt`, a read and the checks on what it read, every [`POLL_INTERVAL`] until the checks hold or the
/// [`delivery_deadline`] has passed since the first attempt; it answers what the passing attempt read.
///
/// # Errors
///
/// Returns the failure of the last attempt.
pub async fn retried<T, F>(mut attempt: F) -> googletest::Result<T>
where
    F: AsyncFnMut() -> googletest::Result<T>,
{
    let deadline = Instant::now() + delivery_deadline();
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(failure) if Instant::now() >= deadline => return Err(failure),
            Err(_) => sleep(POLL_INTERVAL).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::eventually::{Poll, eventually, retried};
    use googletest::prelude::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    #[gtest]
    #[tokio::test]
    async fn a_wait_ends_at_the_first_ready_poll() -> Result<()> {
        let polls = AtomicUsize::new(0);
        let value = eventually(Duration::from_secs(5), || async {
            match polls.fetch_add(1, Ordering::Relaxed) {
                0 | 1 => Poll::Pending("not yet".to_owned()),
                count => Poll::Ready(count),
            }
        })
        .await?;
        verify_that!(value, eq(2))
    }

    #[gtest]
    #[tokio::test]
    async fn a_wait_past_its_bound_reports_the_last_poll() -> Result<()> {
        let result = eventually(Duration::from_millis(300), || async { Poll::<()>::Pending("still empty".to_owned()) }).await;
        let error = result.err().or_fail()?;
        verify_that!(error.last, eq("still empty"))
    }

    #[gtest]
    #[tokio::test]
    async fn a_retried_read_answers_the_first_attempt_whose_checks_hold() -> Result<()> {
        let attempts = AtomicUsize::new(0);
        let seen = retried(async || {
            let count = attempts.fetch_add(1, Ordering::Relaxed);
            verify_that!(count, ge(2))?;
            Ok(count)
        })
        .await?;
        verify_that!(seen, eq(2))
    }
}
