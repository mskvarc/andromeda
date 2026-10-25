//! The bounded loop that runs an operation again after a retryable failure.

use crate::{retry_delay::RetryDelay, retry_number::RetryNumber, schedule::BackoffSchedule};
use std::num::NonZeroU32;

/// Which run of an operation is starting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryAttempt {
    /// The first run, which follows no wait.
    First,
    /// A run after a lost attempt, and the wait the schedule held it for.
    Retry(RetryNumber),
}

/// How often, and how far apart, a failed operation is run again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    /// Runs in all, the first included.
    pub attempts: NonZeroU32,
    /// The wait before each run after the first.
    pub schedule: BackoffSchedule,
}

impl RetryPolicy {
    /// Run `operation`, waiting on `delay` and running it again while it fails
    /// with an error `retryable` accepts and attempts remain.
    ///
    /// `retryable` is told the retry the error would lead to, so a caller can
    /// record the loss. Nothing is waited before the first run, and nothing
    /// after the last.
    ///
    /// # Errors
    ///
    /// The operation's error when `retryable` refuses it, or the last error
    /// once every attempt is spent.
    pub async fn run<T, E>(
        &self,
        delay: &dyn RetryDelay,
        retryable: impl Fn(&E, RetryNumber) -> bool,
        mut operation: impl AsyncFnMut(RetryAttempt) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut attempt = RetryAttempt::First;
        let mut runs = NonZeroU32::MIN;
        loop {
            let retry = RetryNumber::new(runs);
            match operation(attempt).await {
                Err(error) if runs < self.attempts && retryable(&error, retry) => {
                    delay.wait(self.schedule.delay(retry)).await;
                    attempt = RetryAttempt::Retry(retry);
                    runs = runs.saturating_add(1);
                }
                outcome => return outcome,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        growth::Growth,
        retry_delay::RetryDelay,
        retry_number::RetryNumber,
        retry_policy::{RetryAttempt, RetryPolicy},
        schedule::BackoffSchedule,
    };
    use futures::future::BoxFuture;
    use pretty_assertions::assert_eq;
    use std::{
        num::NonZeroU32,
        sync::{
            Mutex,
            atomic::{AtomicU32, Ordering},
        },
        time::Duration,
    };

    /// Three runs, doubling from one second.
    const THREE_RUNS: RetryPolicy = RetryPolicy {
        attempts: NonZeroU32::MIN.saturating_add(2),
        schedule: BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(8), Growth::Doubling),
    };

    /// A wait that returns at once and records what it was asked for.
    #[derive(Default)]
    struct RecordedDelay(Mutex<Vec<Duration>>);

    impl RecordedDelay {
        fn waits(&self) -> Vec<Duration> {
            self.0.lock().map(|waits| waits.clone()).unwrap_or_default()
        }
    }

    impl RetryDelay for RecordedDelay {
        fn wait<'life0, 'async_trait>(&'life0 self, delay: Duration) -> BoxFuture<'async_trait, ()>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            if let Ok(mut waits) = self.0.lock() {
                waits.push(delay);
            }
            Box::pin(async {})
        }
    }

    fn count_run(runs: &AtomicU32) -> u32 {
        runs.fetch_add(1, Ordering::SeqCst) + 1
    }

    #[tokio::test]
    async fn a_retryable_error_is_run_again_until_it_succeeds() {
        let runs = AtomicU32::new(0);
        let delay = RecordedDelay::default();
        let outcome = THREE_RUNS
            .run(
                &delay,
                |error: &&str, _| *error == "contended",
                async |_| {
                    let run = count_run(&runs);
                    if run < 3 { Err("contended") } else { Ok(run) }
                },
            )
            .await;
        assert_eq!(outcome, Ok(3));
        assert_eq!(runs.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn an_error_retried_on_every_attempt_stops_at_the_budget_with_that_error() {
        let runs = AtomicU32::new(0);
        let delay = RecordedDelay::default();
        let outcome: Result<(), &str> = THREE_RUNS
            .run(
                &delay,
                |_: &&str, _| true,
                async |_| {
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
        let delay = RecordedDelay::default();
        let outcome: Result<(), &str> = THREE_RUNS
            .run(
                &delay,
                |error: &&str, _| *error == "contended",
                async |_| {
                    count_run(&runs);
                    Err("refused")
                },
            )
            .await;
        assert_eq!(outcome, Err("refused"));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(delay.waits(), []);
    }

    #[tokio::test]
    async fn the_waits_follow_the_schedule_and_none_precedes_the_first_run_or_follows_the_last() {
        let delay = RecordedDelay::default();
        let outcome: Result<(), &str> = THREE_RUNS.run(&delay, |_: &&str, _| true, async |_| Err("contended")).await;
        assert_eq!(outcome, Err("contended"));
        assert_eq!(
            delay.waits(),
            [
                THREE_RUNS.schedule.delay(RetryNumber::FIRST),
                THREE_RUNS.schedule.delay(RetryNumber::FIRST.next())
            ]
        );
    }

    #[tokio::test]
    async fn the_operation_and_the_classifier_are_told_which_retry_is_running() {
        let delay = RecordedDelay::default();
        let attempts = Mutex::new(Vec::new());
        let classified = Mutex::new(Vec::new());
        let outcome: Result<(), &str> = THREE_RUNS
            .run(
                &delay,
                |_: &&str, retry| {
                    if let Ok(mut classified) = classified.lock() {
                        classified.push(retry.get());
                    }
                    true
                },
                async |attempt| {
                    if let Ok(mut attempts) = attempts.lock() {
                        attempts.push(attempt);
                    }
                    Err("contended")
                },
            )
            .await;
        assert_eq!(outcome, Err("contended"));
        assert_eq!(classified.into_inner().unwrap_or_default(), [1, 2]);
        assert_eq!(
            attempts.into_inner().unwrap_or_default(),
            [
                RetryAttempt::First,
                RetryAttempt::Retry(RetryNumber::FIRST),
                RetryAttempt::Retry(RetryNumber::FIRST.next())
            ]
        );
    }
}
