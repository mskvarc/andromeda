//! Explicit broker readiness checks owned by the test harness.

use andromeda_test_client::{raw_request::RawRequestError, test_client::TestClient};
use http::{Method, StatusCode};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::{sync::OnceCell, time::sleep};

static PROCESS_READINESS: OnceCell<()> = OnceCell::const_new();

/// Timing policy for polling the broker health endpoint.
#[derive(Clone, Copy, Debug)]
pub struct ReadinessPolicy {
    timeout: Duration,
    interval: Duration,
    request_timeout: Duration,
}

impl ReadinessPolicy {
    /// Construct a readiness policy with an overall timeout and retry interval.
    #[must_use]
    pub const fn new(timeout: Duration, interval: Duration, request_timeout: Duration) -> ReadinessPolicy {
        ReadinessPolicy {
            timeout,
            interval,
            request_timeout,
        }
    }
}

impl Default for ReadinessPolicy {
    fn default() -> Self {
        ReadinessPolicy::new(Duration::from_secs(10), Duration::from_millis(100), Duration::from_secs(1))
    }
}

/// Poll broker readiness once for the current test process.
pub async fn wait_until_ready_once(client: &TestClient, policy: ReadinessPolicy) -> Result<(), ReadinessError> {
    PROCESS_READINESS
        .get_or_try_init(|| async { wait_until_ready(client, policy).await })
        .await
        .map(|()| ())
}

/// Poll the broker health endpoint according to an explicit timing policy.
pub async fn wait_until_ready(client: &TestClient, policy: ReadinessPolicy) -> Result<(), ReadinessError> {
    let started = Instant::now();
    let mut attempts = 0_u32;
    loop {
        attempts = attempts.saturating_add(1);
        match client.raw(Method::GET, "/ngsi-ld/v1/health").timeout(policy.request_timeout).send().await {
            Ok(response) if response.status.is_success() => return Ok(()),
            Ok(response) if started.elapsed() >= policy.timeout => {
                return Err(ReadinessError::UnexpectedStatus {
                    attempts,
                    status: response.status,
                });
            }
            Ok(_response) => {}
            Err(source) if started.elapsed() >= policy.timeout => {
                return Err(ReadinessError::Unreachable { attempts, source });
            }
            Err(_source) => {}
        }
        sleep(policy.interval).await;
    }
}

/// Broker readiness failure with attempt context.
#[derive(Debug, Error)]
pub enum ReadinessError {
    /// The broker could not be reached before the readiness deadline.
    #[error("broker remained unreachable after {attempts} readiness attempts: {source}")]
    Unreachable {
        /// Number of attempted health requests.
        attempts: u32,
        /// Final transport failure.
        source: RawRequestError,
    },
    /// The broker kept returning a non-success health status.
    #[error("broker returned health status {status} after {attempts} readiness attempts")]
    UnexpectedStatus {
        /// Number of attempted health requests.
        attempts: u32,
        /// Final health response status.
        status: StatusCode,
    },
}
