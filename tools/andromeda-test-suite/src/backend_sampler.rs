//! Sampling a server's client backends while a workload runs, for a test bounding how many backends a broker opens or
//! keeps: the peak, the trough and the median of the samples.

use crate::server_backends::{BackendScope, ServerBackends, ServerBackendsError};
use std::time::Duration;
use tokio::{
    select,
    sync::oneshot,
    task::{JoinError, JoinHandle},
    time::sleep,
};

/// How often the backends are counted.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// The client backends counted over a sampling run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSeries {
    /// The highest count.
    pub peak: u64,
    /// The lowest count.
    pub trough: u64,
    /// The median count.
    pub median: u64,
    /// How many counts were taken.
    pub samples: usize,
}

impl BackendSeries {
    /// The series of `samples`, or `None` when there are none.
    #[must_use]
    pub fn of(samples: &[u64]) -> Option<BackendSeries> {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let sorted = sorted;
        Some(BackendSeries {
            peak: *sorted.last()?,
            trough: *sorted.first()?,
            median: *sorted.get(sorted.len() / 2)?,
            samples: sorted.len(),
        })
    }
}

/// Why a sampling run gave no series.
#[derive(Debug, thiserror::Error)]
pub enum BackendSamplerError {
    /// A count failed: a series missing a sample could hide its peak.
    #[error(transparent)]
    Count(#[from] ServerBackendsError),
    /// The sampling task ended abnormally.
    #[error("the sampling task ended abnormally")]
    Task(#[from] JoinError),
    /// The run ended before its first count.
    #[error("the sampling run took no sample")]
    Empty,
}

/// A sampling run in progress.
#[derive(Debug)]
pub struct BackendSampler {
    /// Ends the run.
    stop: oneshot::Sender<()>,
    /// The run, answering its samples.
    task: JoinHandle<Result<Vec<u64>, ServerBackendsError>>,
}

impl BackendSampler {
    /// Starts counting the backends `scope` covers on `server`.
    #[must_use]
    pub fn start(server: ServerBackends, scope: BackendScope) -> BackendSampler {
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut samples = Vec::new();
            loop {
                samples.push(server.count(&scope).await?);
                select! {
                    _ = &mut stopped => return Ok(samples),
                    () = sleep(SAMPLE_INTERVAL) => {}
                }
            }
        });
        BackendSampler { stop, task }
    }

    /// Ends the run and answers its series.
    ///
    /// # Errors
    ///
    /// Returns [`BackendSamplerError`] when a count failed or no count was taken.
    pub async fn stop(self) -> Result<BackendSeries, BackendSamplerError> {
        // The task ends at its next sample when the receiver is gone, so a send that finds it ended already is not a
        // failure of its own: the task's result says why it ended.
        let _ended = self.stop.send(());
        let samples = self.task.await??;
        BackendSeries::of(&samples).ok_or(BackendSamplerError::Empty)
    }

    /// The series of the backends `scope` covers on `server` over `window`.
    ///
    /// # Errors
    ///
    /// Returns [`BackendSamplerError`] when a count failed or no count was taken.
    pub async fn over(server: ServerBackends, scope: BackendScope, window: Duration) -> Result<BackendSeries, BackendSamplerError> {
        let sampler = BackendSampler::start(server, scope);
        sleep(window).await;
        sampler.stop().await
    }
}

#[cfg(test)]
mod tests {
    use crate::backend_sampler::BackendSeries;
    use googletest::prelude::*;

    #[gtest]
    fn a_series_holds_its_peak_trough_and_median() {
        expect_that!(
            BackendSeries::of(&[4, 9, 2, 7, 5]),
            some(eq(BackendSeries {
                peak: 9,
                trough: 2,
                median: 5,
                samples: 5
            }))
        );
    }

    #[gtest]
    fn no_samples_make_no_series() {
        expect_that!(BackendSeries::of(&[]), none());
    }
}
