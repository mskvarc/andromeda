//! A bounded load: workers sending requests one after another until a deadline, and the tally of how every request
//! resolved and how long it took.

use crate::{
    connection_outcome::ConnectionOutcome,
    load_outcome::{LoadOutcome, RetryAfter},
};
use futures::future::join_all;
use http::StatusCode;
use std::{
    collections::BTreeMap,
    fmt,
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::time::Instant;

/// How every request of a load resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// The requests answered, by status.
    answered: BTreeMap<StatusCode, usize>,
    /// The 503 answers that named no delay to ask again after.
    unpaced_unavailability: usize,
    /// The requests left without an answer, by what became of them.
    unanswered: BTreeMap<ConnectionOutcome, usize>,
    /// Every request's latency, ascending.
    latencies: Vec<Duration>,
}

impl LoadReport {
    /// The report of requests that resolved as `samples` say, each with its latency.
    #[must_use]
    pub fn of(samples: impl IntoIterator<Item = (Duration, LoadOutcome)>) -> LoadReport {
        let mut report = LoadReport::default();
        for (latency, outcome) in samples {
            report.latencies.push(latency);
            match outcome {
                LoadOutcome::Answered { status, retry_after } => {
                    *report.answered.entry(status).or_default() += 1;
                    if status == StatusCode::SERVICE_UNAVAILABLE && !matches!(retry_after, Some(RetryAfter::Delay(_))) {
                        report.unpaced_unavailability += 1;
                    }
                }
                LoadOutcome::Unanswered(connection) => *report.unanswered.entry(connection).or_default() += 1,
            }
        }
        report.latencies.sort_unstable();
        report
    }

    /// How many requests were sent.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.latencies.len()
    }

    /// How many requests were answered `status`.
    #[must_use]
    pub fn answered(&self, status: StatusCode) -> usize {
        self.answered.get(&status).copied().unwrap_or_default()
    }

    /// How many requests got no success: an error status or no answer.
    #[must_use]
    pub fn unsuccessful(&self) -> usize {
        self.total()
            - self
                .answered
                .iter()
                .filter(|(status, _)| status.is_success())
                .map(|(_, count)| count)
                .sum::<usize>()
    }

    /// How many requests were answered with an error status other than 503.
    #[must_use]
    pub fn errors_besides_unavailability(&self) -> usize {
        self.answered
            .iter()
            .filter(|(status, _)| !status.is_success() && **status != StatusCode::SERVICE_UNAVAILABLE)
            .map(|(_, count)| count)
            .sum()
    }

    /// How many 503 answers named no delay to ask again after.
    #[must_use]
    pub const fn unpaced_unavailability(&self) -> usize {
        self.unpaced_unavailability
    }

    /// How many requests got no answer.
    #[must_use]
    pub fn unanswered(&self) -> usize {
        self.unanswered.values().sum()
    }

    /// The slowest request's latency.
    #[must_use]
    pub fn slowest(&self) -> Duration {
        self.latencies.last().copied().unwrap_or_default()
    }

    /// The latency `percent` of the requests stayed within.
    #[must_use]
    pub fn percentile(&self, percent: usize) -> Duration {
        let index = self.latencies.len().saturating_sub(1).saturating_mul(percent.min(100)) / 100;
        self.latencies.get(index).copied().unwrap_or_default()
    }
}

// Hand-written: the tally reads as one line of a failure message.
impl fmt::Display for LoadReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} requests, answered {:?}, {} 503 without a delay, unanswered {:?}, p50 {:?}, p99 {:?}, slowest {:?}",
            self.total(),
            self.answered,
            self.unpaced_unavailability,
            self.unanswered,
            self.percentile(50),
            self.percentile(99),
            self.slowest()
        )
    }
}

/// Runs `workers` workers, each sending `workload`'s request after request until `duration` has passed, and tallies
/// how every request resolved. `workload` is given each request's number, counted across every worker.
pub async fn run_load<F, Fut>(workers: usize, duration: Duration, workload: F) -> LoadReport
where
    F: Fn(usize) -> Fut + Sync,
    Fut: Future<Output = LoadOutcome>,
{
    let deadline = Instant::now() + duration;
    let numbers = AtomicUsize::new(0);
    let workload = &workload;
    let numbers = &numbers;
    let runs = (0..workers).map(|_| async move {
        let mut resolved = Vec::new();
        while Instant::now() < deadline {
            let number = numbers.fetch_add(1, Ordering::Relaxed);
            let started = Instant::now();
            let outcome = workload(number).await;
            resolved.push((started.elapsed(), outcome));
        }
        resolved
    });
    LoadReport::of(join_all(runs).await.into_iter().flatten())
}

#[cfg(test)]
mod tests {
    use crate::{
        connection_outcome::ConnectionOutcome,
        load_outcome::{LoadOutcome, RetryAfter},
        load_run::{LoadReport, run_load},
    };
    use googletest::prelude::*;
    use http::StatusCode;
    use std::time::Duration;
    use tokio::time::sleep;

    /// An answer of `status` carrying `retry_after`.
    const fn answered(status: StatusCode, retry_after: Option<RetryAfter>) -> LoadOutcome {
        LoadOutcome::Answered { status, retry_after }
    }

    #[gtest]
    fn a_report_tallies_statuses_unpaced_503s_and_unanswered_requests_apart() {
        let delay = Some(RetryAfter::Delay(Duration::from_secs(1)));
        let report = LoadReport::of([
            (Duration::from_millis(5), answered(StatusCode::OK, None)),
            (Duration::from_millis(9), answered(StatusCode::SERVICE_UNAVAILABLE, delay)),
            (Duration::from_millis(7), answered(StatusCode::SERVICE_UNAVAILABLE, None)),
            (Duration::from_millis(3), answered(StatusCode::INTERNAL_SERVER_ERROR, None)),
            (Duration::from_millis(1), LoadOutcome::Unanswered(ConnectionOutcome::Refused)),
        ]);
        expect_that!(report.total(), eq(5));
        expect_that!(report.answered(StatusCode::SERVICE_UNAVAILABLE), eq(2));
        expect_that!(report.unsuccessful(), eq(4));
        expect_that!(report.errors_besides_unavailability(), eq(1));
        expect_that!(report.unpaced_unavailability(), eq(1));
        expect_that!(report.unanswered(), eq(1));
        expect_that!(report.slowest(), eq(Duration::from_millis(9)));
        expect_that!(report.percentile(50), eq(Duration::from_millis(5)));
    }

    #[gtest]
    #[tokio::test]
    async fn a_load_runs_every_worker_until_the_deadline() {
        let report = run_load(3, Duration::from_millis(50), |_| async {
            sleep(Duration::from_millis(5)).await;
            answered(StatusCode::OK, None)
        })
        .await;
        expect_that!(report.total(), ge(3));
        expect_that!(report.unsuccessful(), eq(0));
    }
}
