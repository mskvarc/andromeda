//! The one loop that runs an operation again after the database refused it
//! under contention.
//!
//! Deadlocks, serialization failures, refused locks and exhausted connection
//! slots clear on their own, so the operation that met one is run again a
//! bounded number of times. Each caller owns what counts as contention, how
//! many runs it gets and how long it pauses between them; only the loop is
//! shared.

use std::{num::NonZeroU32, time::Duration};
use tokio::time::sleep;

/// How long to pause before an operation is run again.
pub trait RetryPause {
    /// The pause after `lost_attempts` runs have been refused, at least one.
    fn after(&self, lost_attempts: NonZeroU32) -> Duration;
}

/// A fixed pause between runs.
impl RetryPause for Duration {
    fn after(&self, _lost_attempts: NonZeroU32) -> Duration {
        *self
    }
}

/// How often, and how far apart, an operation refused under contention is run
/// again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentionRetry<P> {
    /// Runs in all, the first included.
    pub attempts: NonZeroU32,
    /// The pause before each run after the first.
    pub pause: P,
}

impl<P: RetryPause> ContentionRetry<P> {
    /// Run `operation`, running it again after the [`pause`](Self::pause)
    /// while it fails with an error `retryable` recognises and attempts
    /// remain.
    ///
    /// # Errors
    ///
    /// Returns the operation's error when `retryable` does not recognise it,
    /// or the last recognised error once every attempt is spent.
    pub async fn run<T, E>(self, retryable: impl Fn(&E) -> bool, mut operation: impl AsyncFnMut() -> Result<T, E>) -> Result<T, E> {
        let mut lost_attempts = NonZeroU32::MIN;
        loop {
            match operation().await {
                Err(error) if lost_attempts < self.attempts && retryable(&error) => {
                    sleep(self.pause.after(lost_attempts)).await;
                    lost_attempts = lost_attempts.saturating_add(1);
                }
                outcome => return outcome,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::contention_retry::{ContentionRetry, RetryPause};
    use pretty_assertions::assert_eq;
    use std::{
        num::NonZeroU32,
        sync::atomic::{AtomicU32, Ordering},
        time::Duration,
    };

    /// Three attempts with no pause between them.
    const THREE_IMMEDIATE: ContentionRetry<Duration> = ContentionRetry {
        attempts: NonZeroU32::MIN.saturating_add(2),
        pause: Duration::ZERO,
    };

    /// A pause that records how many attempts were lost before each wait.
    struct RecordedPause<'a>(&'a AtomicU32);

    impl RetryPause for RecordedPause<'_> {
        fn after(&self, lost_attempts: NonZeroU32) -> Duration {
            let recorded = self.0.load(Ordering::SeqCst);
            self.0.store(recorded * 10 + lost_attempts.get(), Ordering::SeqCst);
            Duration::ZERO
        }
    }

    /// Count one run and return its ordinal.
    fn count_run(runs: &AtomicU32) -> u32 {
        runs.fetch_add(1, Ordering::SeqCst) + 1
    }

    #[tokio::test]
    async fn a_retryable_error_is_run_again_until_it_succeeds() {
        let runs = AtomicU32::new(0);
        let outcome = THREE_IMMEDIATE
            .run(
                |error: &&str| *error == "contended",
                || async {
                    let run = count_run(&runs);
                    if run < 3 { Err("contended") } else { Ok(run) }
                },
            )
            .await;
        assert_eq!(outcome, Ok(3));
        assert_eq!(runs.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn an_error_retried_on_every_attempt_stops_at_the_bound_with_that_error() {
        let runs = AtomicU32::new(0);
        let outcome: Result<(), &str> = THREE_IMMEDIATE
            .run(
                |_: &&str| true,
                || async {
                    count_run(&runs);
                    Err("contended")
                },
            )
            .await;
        assert_eq!(outcome, Err("contended"));
        assert_eq!(runs.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn an_error_the_caller_does_not_retry_is_returned_from_its_first_run() {
        let runs = AtomicU32::new(0);
        let outcome: Result<(), &str> = THREE_IMMEDIATE
            .run(
                |error: &&str| *error == "contended",
                || async {
                    count_run(&runs);
                    Err("refused")
                },
            )
            .await;
        assert_eq!(outcome, Err("refused"));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_pause_is_told_how_many_attempts_were_lost() {
        let waits = AtomicU32::new(0);
        let retry = ContentionRetry {
            attempts: NonZeroU32::MIN.saturating_add(2),
            pause: RecordedPause(&waits),
        };
        let outcome: Result<(), &str> = retry.run(|_: &&str| true, || async { Err("contended") }).await;
        assert_eq!(outcome, Err("contended"));
        assert_eq!(waits.load(Ordering::SeqCst), 12);
    }

    #[test]
    fn a_duration_pauses_the_same_time_after_every_lost_attempt() {
        let pause = Duration::from_secs(2);
        assert_eq!(pause.after(NonZeroU32::MIN), Duration::from_secs(2));
        assert_eq!(pause.after(NonZeroU32::MIN.saturating_add(3)), Duration::from_secs(2));
    }
}
