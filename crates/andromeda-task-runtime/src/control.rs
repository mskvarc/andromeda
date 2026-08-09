//! Cancellation context and coalescing run-now control.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

/// Context supplied to one bounded scheduled execution.
#[derive(Clone, Debug)]
pub struct ScheduledExecution {
    /// Task-local cooperative cancellation.
    cancellation: CancellationToken,
    /// Fixed wall-clock instant for the bounded execution.
    cutoff: SystemTime,
}

/// Context supplied to one bounded durable drain execution.
#[derive(Clone, Debug)]
pub struct DrainExecution {
    /// Task-local cooperative cancellation.
    cancellation: CancellationToken,
    /// Fixed wall-clock instant for the bounded claim-and-process pass.
    cutoff: SystemTime,
}

impl DrainExecution {
    /// Construct one drain execution context.
    pub(crate) const fn new(cancellation: CancellationToken, cutoff: SystemTime) -> DrainExecution {
        DrainExecution { cancellation, cutoff }
    }

    /// Return the cancellation token for cooperative active shutdown.
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Return the wall-clock cutoff fixed for this bounded pass.
    pub const fn cutoff(&self) -> SystemTime {
        self.cutoff
    }
}

impl ScheduledExecution {
    /// Construct one scheduled execution context.
    pub(crate) const fn new(cancellation: CancellationToken, cutoff: SystemTime) -> ScheduledExecution {
        ScheduledExecution { cancellation, cutoff }
    }

    /// Return the cancellation token for cooperative active shutdown.
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Return the wall-clock cutoff fixed for this execution.
    pub const fn cutoff(&self) -> SystemTime {
        self.cutoff
    }
}

/// Context supplied to one resident execution attempt.
#[derive(Clone, Debug)]
pub struct ResidentExecution {
    /// Task-local cooperative cancellation.
    cancellation: CancellationToken,
    /// One-shot transition from startup into resident running state.
    readiness: Arc<ResidentReadiness>,
}

impl ResidentExecution {
    /// Construct one resident execution context.
    pub(crate) const fn new(cancellation: CancellationToken, readiness: Arc<ResidentReadiness>) -> ResidentExecution {
        ResidentExecution { cancellation, readiness }
    }

    /// Return the cancellation token for cooperative shutdown.
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Report that startup work completed and the resident resource is running.
    pub fn mark_running(&self) {
        self.readiness.running.store(true, Ordering::Release);
        self.readiness.changed.notify_waiters();
    }
}

/// One resident attempt's explicit transition into running state.
#[derive(Debug, Default)]
pub(crate) struct ResidentReadiness {
    /// Whether readiness has been reported.
    running: AtomicBool,
    /// Notification waking the supervising driver.
    changed: Notify,
}

impl ResidentReadiness {
    /// Return whether the operation reported its running transition.
    pub(crate) fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Wait until the operation reports that its resident resource is running.
    pub(crate) async fn wait(&self) {
        loop {
            let notified = self.changed.notified();
            if self.is_running() {
                return;
            }
            notified.await;
        }
    }
}

/// Result of signalling a scheduled task to execute promptly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunNowRequest {
    /// A prompt execution was queued.
    Scheduled,
    /// A prompt execution was already pending and the signals coalesced.
    Coalesced,
    /// The scheduled task has stopped and no execution can be requested.
    Closed,
}

/// Capability to request prompt execution of one scheduled task.
#[derive(Clone, Debug)]
pub struct RunNowHandle {
    /// Bounded signal sender whose single slot supplies coalescing.
    sender: mpsc::Sender<()>,
}

impl RunNowHandle {
    /// Request one prompt execution, coalescing with an existing request.
    pub fn request(&self) -> RunNowRequest {
        match self.sender.try_send(()) {
            Ok(()) => RunNowRequest::Scheduled,
            Err(mpsc::error::TrySendError::Full(())) => RunNowRequest::Coalesced,
            Err(mpsc::error::TrySendError::Closed(())) => RunNowRequest::Closed,
        }
    }
}

/// Construct a one-slot prompt-execution channel.
pub(crate) fn run_now_channel() -> (RunNowHandle, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel(1);
    (RunNowHandle { sender }, receiver)
}
