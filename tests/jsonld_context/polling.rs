//! Waiting for an effect the broker applies after it has answered.
//!
//! A downloaded `@context` is recorded durably after the request that fetched
//! it returns, and notifications are delivered asynchronously. A test waits for
//! such an effect by asking again until it holds or a deadline passes, never by
//! sleeping a fixed time.

use std::{future::Future, time::Duration};
use tokio::time::{Instant, sleep};

/// How long an asynchronous effect is waited for before the test fails.
pub const EFFECT_DEADLINE: Duration = Duration::from_secs(20);

/// How long to wait between two probes.
const PROBE_INTERVAL: Duration = Duration::from_millis(100);

/// Probe until `probe` returns `Some`, and return that value; fail the test
/// naming `what` when [`EFFECT_DEADLINE`] passes first.
pub async fn eventually<T, F, P>(what: &str, mut probe: P) -> T
where
    P: FnMut() -> F,
    F: Future<Output = Option<T>>,
{
    let deadline = Instant::now() + EFFECT_DEADLINE;
    loop {
        if let Some(value) = probe().await {
            return value;
        }
        assert!(Instant::now() < deadline, "{what} did not happen within {EFFECT_DEADLINE:?}");
        sleep(PROBE_INTERVAL).await;
    }
}
