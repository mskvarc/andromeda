//! One bounded run of an operation under a [`RetryPolicy`], driven by the
//! caller's own loop.
//!
//! The caller runs each attempt inline and hands its outcome to
//! [`RetryRun::settle`], which decides whether the run ends there or waits and
//! asks for another attempt. An attempt is ordinary code in the caller's async
//! function, so it borrows and mutates the caller's state across attempts, and
//! the compiler proves the caller's future `Send` from that code alone, with no
//! closure whose futures it would have to prove `Send` for every lifetime.

use crate::{
    retry_delay::RetryDelay,
    retry_number::RetryNumber,
    retry_policy::{RetryAttempt, RetryPolicy},
};
use std::{num::NonZeroU32, ops::ControlFlow};

/// The attempts of one operation under a [`RetryPolicy`].
pub struct RetryRun<'d> {
    /// The budget and the schedule the run follows.
    policy: RetryPolicy,
    /// The timer that waits out each delay.
    delay: &'d dyn RetryDelay,
    /// The attempt about to start.
    attempt: RetryAttempt,
    /// Attempts started so far, the one about to start included.
    runs: NonZeroU32,
}

impl RetryRun<'_> {
    /// A run of `policy` before its first attempt, whose waits `delay`
    /// measures.
    #[must_use]
    pub const fn new(policy: RetryPolicy, delay: &dyn RetryDelay) -> RetryRun<'_> {
        RetryRun {
            policy,
            delay,
            attempt: RetryAttempt::First,
            runs: NonZeroU32::MIN,
        }
    }

    /// The attempt about to start.
    #[must_use]
    pub const fn attempt(&self) -> RetryAttempt {
        self.attempt
    }

    /// Decide what follows `outcome`, the result of the attempt that just ran.
    ///
    /// The run ends with `outcome` when it succeeded, when `retryable` refuses
    /// its error, or when the budget is spent. Otherwise the run waits the
    /// schedule's delay and continues, and the next attempt is a retry.
    /// `retryable` is told the retry the error would lead to, so a caller can
    /// record the loss. Nothing is waited after the last attempt.
    pub async fn settle<T, E>(&mut self, outcome: Result<T, E>, retryable: impl Fn(&E, RetryNumber) -> bool) -> ControlFlow<Result<T, E>> {
        let retry = RetryNumber::new(self.runs);
        match outcome {
            Err(error) if self.runs < self.policy.attempts && retryable(&error, retry) => {
                drop(error);
                self.delay.wait(self.policy.schedule.delay(retry)).await;
                self.attempt = RetryAttempt::Retry(retry);
                self.runs = self.runs.saturating_add(1);
                ControlFlow::Continue(())
            }
            settled => ControlFlow::Break(settled),
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
    use std::{num::NonZeroU32, ops::ControlFlow, sync::Mutex, time::Duration};

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
            self.0.lock().map_or_default(|waits| waits.clone())
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

    /// Drive `THREE_RUNS` over the outcomes `attempt` yields in turn, returning
    /// the settled outcome and every attempt that ran.
    async fn drive<T: Send, E: Send>(
        delay: &RecordedDelay,
        retryable: impl Fn(&E, RetryNumber) -> bool + Sync,
        mut attempt: impl FnMut(u32) -> Result<T, E> + Send,
    ) -> (Result<T, E>, Vec<RetryAttempt>) {
        let mut run = THREE_RUNS.start(delay);
        let mut attempts = Vec::new();
        let mut ran = 0;
        loop {
            attempts.push(run.attempt());
            ran += 1;
            let outcome = attempt(ran);
            if let ControlFlow::Break(settled) = run.settle(outcome, &retryable).await {
                return (settled, attempts);
            }
        }
    }

    #[tokio::test]
    async fn a_retryable_error_is_run_again_until_it_succeeds() {
        let delay = RecordedDelay::default();
        let (outcome, attempts) = drive(
            &delay,
            |error: &&str, _| *error == "contended",
            |run| if run < 3 { Err("contended") } else { Ok(run) },
        )
        .await;
        assert_eq!(outcome, Ok(3));
        assert_eq!(attempts.len(), 3);
    }

    #[tokio::test]
    async fn an_error_retried_on_every_attempt_stops_at_the_budget_with_that_error() {
        let delay = RecordedDelay::default();
        let (outcome, attempts) = drive::<(), _>(&delay, |_: &&str, _| true, |_| Err("contended")).await;
        assert_eq!(outcome, Err("contended"));
        assert_eq!(attempts.len(), 3);
    }

    #[tokio::test]
    async fn an_error_the_caller_does_not_retry_is_returned_from_its_first_run() {
        let delay = RecordedDelay::default();
        let (outcome, attempts) = drive::<(), _>(&delay, |error: &&str, _| *error == "contended", |_| Err("refused")).await;
        assert_eq!(outcome, Err("refused"));
        assert_eq!(attempts.len(), 1);
        assert_eq!(delay.waits(), []);
    }

    #[tokio::test]
    async fn the_waits_follow_the_schedule_and_none_precedes_the_first_run_or_follows_the_last() {
        let delay = RecordedDelay::default();
        let (outcome, _) = drive::<(), _>(&delay, |_: &&str, _| true, |_| Err("contended")).await;
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
    async fn the_attempt_and_the_classifier_are_told_which_retry_is_running() {
        let delay = RecordedDelay::default();
        let classified = Mutex::new(Vec::new());
        let (outcome, attempts) = drive::<(), _>(
            &delay,
            |_: &&str, retry| {
                if let Ok(mut classified) = classified.lock() {
                    classified.push(retry.get());
                }
                true
            },
            |_| Err("contended"),
        )
        .await;
        assert_eq!(outcome, Err("contended"));
        assert_eq!(classified.into_inner().unwrap_or_default(), [1, 2]);
        assert_eq!(
            attempts,
            [
                RetryAttempt::First,
                RetryAttempt::Retry(RetryNumber::FIRST),
                RetryAttempt::Retry(RetryNumber::FIRST.next())
            ]
        );
    }
}
