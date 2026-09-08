//! Panic-aware isolated runner preserving test and cleanup outcomes.

use crate::{
    configuration::TestRunConfiguration,
    environment::{TestEnvironment, TestEnvironmentError},
    identifiers::TestSuiteName,
};
use futures::FutureExt;
use std::{any::Any, future::Future, panic::AssertUnwindSafe};
use thiserror::Error;

/// Run one prefix-isolated test and always attempt explicit cleanup.
///
/// # Errors
///
/// Returns [`RunIsolatedError::Environment`] when the environment cannot be built or its cleanup fails on its own, [`RunIsolatedError::Test`] when the test body returns an error or panics, and [`RunIsolatedError::TestAndCleanup`] when the body and the cleanup both fail.
pub async fn run_isolated<F, Fut, E>(configuration: &TestRunConfiguration, suite: &TestSuiteName, test: F) -> Result<(), RunIsolatedError<E>>
where
    F: FnOnce(&TestEnvironment) -> Fut,
    Fut: Future<Output = Result<(), E>>,
{
    let environment = TestEnvironment::isolated(configuration, suite).await.map_err(RunIsolatedError::Environment)?;
    let test_result = AssertUnwindSafe(test(&environment)).catch_unwind().await;
    let cleanup_result = environment.finish().await;
    combine_results(test_result, cleanup_result)
}

fn combine_results<E>(test: Result<Result<(), E>, Box<dyn Any + Send>>, cleanup: Result<(), TestEnvironmentError>) -> Result<(), RunIsolatedError<E>> {
    let test = match test {
        Ok(result) => result.map_err(TestFailure::Returned),
        Err(payload) => Err(TestFailure::Panicked(PanicReport::from_payload(payload.as_ref()))),
    };
    match (test, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(test), Ok(())) => Err(RunIsolatedError::Test(test)),
        (Ok(()), Err(cleanup)) => Err(RunIsolatedError::Environment(cleanup)),
        (Err(test), Err(cleanup)) => Err(RunIsolatedError::TestAndCleanup { test, cleanup }),
    }
}

/// Test-body failure retaining returned errors and panics as distinct cases.
#[derive(Debug, Error)]
pub enum TestFailure<E> {
    /// The test body returned an error.
    #[error("test body returned an error")]
    Returned(E),
    /// The test body panicked.
    #[error("test body panicked: {0}")]
    Panicked(PanicReport),
}

/// Human-readable panic payload retained after cleanup.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{message}")]
pub struct PanicReport {
    message: String,
}

impl PanicReport {
    fn from_payload(payload: &(dyn Any + Send)) -> PanicReport {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&'static str>().copied())
            .unwrap_or("non-text panic payload")
            .to_owned();
        PanicReport { message }
    }
}

/// Combined isolated-test execution and cleanup failure.
#[derive(Debug, Error)]
pub enum RunIsolatedError<E> {
    /// Environment allocation or cleanup failed without a test-body failure.
    #[error("isolated test environment failed: {0}")]
    Environment(TestEnvironmentError),
    /// The test body failed and cleanup succeeded.
    #[error("isolated test failed: {0}")]
    Test(TestFailure<E>),
    /// Both the test body and cleanup failed.
    #[error("isolated test failed ({test}) and cleanup failed ({cleanup})")]
    TestAndCleanup {
        /// Test-body failure.
        test: TestFailure<E>,
        /// Cleanup failure.
        cleanup: TestEnvironmentError,
    },
}
