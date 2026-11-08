//! Decoding a large request payload beside the asynchronous executor.
//!
//! Parsing a payload into a JSON tree and decoding it into its domain type is
//! pure CPU work proportional to the payload. Run inside an asynchronous task,
//! a payload of tens of megabytes holds one executor worker for seconds, and
//! every unrelated request queued on that worker waits with it. A payload at or
//! above the threshold the HTTP stack publishes is therefore decoded on the
//! blocking pool, and a smaller one inline, where a thread hand-off would cost
//! more than the decode.

use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use std::{num::NonZeroUsize, sync::Arc};
use thiserror::Error;
use tokio::{
    sync::{AcquireError, Semaphore},
    task::{JoinError, spawn_blocking},
};

/// How a request payload is decoded beside the executor, carried as a request
/// extension: the payload size from which it leaves the executor, and the
/// permits every such decode of the process shares.
///
/// A decode costs memory in proportion to its payload while it runs, so the
/// number running at once is capped: a payload waiting for a permit holds
/// only its bytes. A request carrying no policy (a router assembled without the
/// stack, as in a unit test) decodes every payload inline.
#[derive(Clone, Debug)]
pub struct BlockingDecodePolicy {
    /// The payload size, in bytes, from which a decode leaves the executor.
    threshold: usize,
    /// The permits the decodes beside the executor share.
    permits: Arc<Semaphore>,
}

impl BlockingDecodePolicy {
    /// A policy decoding payloads of `threshold` bytes or more beside the
    /// executor, at most `concurrent` of them at once.
    #[must_use]
    pub fn new(threshold: usize, concurrent: NonZeroUsize) -> BlockingDecodePolicy {
        BlockingDecodePolicy {
            threshold,
            permits: Arc::new(Semaphore::new(concurrent.get())),
        }
    }

    /// The payload size from which a decode leaves the executor.
    #[must_use]
    pub const fn threshold_bytes(&self) -> usize {
        self.threshold
    }
}

/// The decode beside the executor ended without an outcome.
#[derive(Debug, Error)]
pub enum DecodeTaskFailure {
    /// The blocking task panicked or the runtime shut down beneath it.
    #[error("the request payload decoding task did not complete: {0}")]
    Join(#[from] JoinError),
    /// The decode permits were closed, which only a shutdown does.
    #[error("the request payload decoding permits are closed: {0}")]
    PermitsClosed(#[from] AcquireError),
}

impl DecodeTaskFailure {
    /// The clause 5.5.6 `InternalError` this failure is answered with.
    #[must_use]
    pub fn report(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::REQUEST_DECODING)
    }
}

/// Run `decode` over a payload of `payload_bytes`: on the blocking pool, once a
/// permit is free, when the payload reaches the policy's threshold, and inline
/// otherwise.
///
/// # Errors
///
/// Returns [`DecodeTaskFailure`] when the blocking task ends without handing
/// back the decode's outcome.
pub async fn decode_payload<T, F>(policy: Option<&BlockingDecodePolicy>, payload_bytes: usize, decode: F) -> Result<T, DecodeTaskFailure>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    match policy {
        Some(policy) if payload_bytes >= policy.threshold => {
            // Held until the decode hands its outcome back.
            let _permit = policy.permits.acquire().await?;
            Ok(spawn_blocking(decode).await?)
        }
        Some(_) | None => Ok(decode()),
    }
}

#[cfg(test)]
mod tests {
    use crate::blocking_decode::{BlockingDecodePolicy, decode_payload};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };
    use tokio::task::JoinSet;

    /// A policy leaving the executor from `threshold` bytes, one decode at a
    /// time.
    fn policy(threshold: usize) -> BlockingDecodePolicy {
        BlockingDecodePolicy::new(threshold, NonZeroUsize::MIN)
    }

    /// The name of the thread a decode ran on.
    async fn decoding_thread(policy: Option<&BlockingDecodePolicy>, payload_bytes: usize) -> Option<String> {
        decode_payload(policy, payload_bytes, || thread::current().name().map(str::to_owned))
            .await
            .expect("the decode completes")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_payload_below_the_threshold_is_decoded_on_the_calling_thread() {
        let caller = thread::current().name().map(str::to_owned);
        pretty_assertions::assert_eq!(decoding_thread(Some(&policy(1024)), 1023).await, caller);
        pretty_assertions::assert_eq!(decoding_thread(None, usize::MAX).await, caller);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_payload_at_the_threshold_is_decoded_on_another_thread() {
        let caller = thread::current().name().map(str::to_owned);
        assert_ne!(decoding_thread(Some(&policy(1024)), 1024).await, caller);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_panicking_decode_is_an_internal_failure() {
        let failure = decode_payload(Some(&policy(0)), 1, || -> u8 { panic!("decoder bug") })
            .await
            .expect_err("the panic is caught");
        pretty_assertions::assert_eq!(failure.report().error_type(), NgsiLdErrorType::InternalError);
    }

    /// No more decodes run beside the executor at once than the policy has
    /// permits for, however many payloads arrive together.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_decodes_never_exceed_the_permits() {
        let two = BlockingDecodePolicy::new(0, NonZeroUsize::MIN.saturating_add(1));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut decodes = JoinSet::new();
        for _ in 0..8 {
            let (two, running, peak) = (two.clone(), Arc::clone(&running), Arc::clone(&peak));
            decodes.spawn(async move {
                decode_payload(Some(&two), 1, move || {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(20));
                    running.fetch_sub(1, Ordering::SeqCst);
                })
                .await
            });
        }
        while let Some(decoded) = decodes.join_next().await {
            decoded.expect("the task completes").expect("the decode completes");
        }
        assert!(peak.load(Ordering::SeqCst) <= 2, "{} decodes ran at once", peak.load(Ordering::SeqCst));
    }
}
