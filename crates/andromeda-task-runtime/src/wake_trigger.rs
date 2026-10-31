//! Coalescing drain wake control and the validated fallback policy.

use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use std::time::Duration;
use tokio::sync::mpsc;

/// Result of signalling a drain task to inspect durable work promptly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WakeRequest {
    /// A prompt drain pass was queued.
    Scheduled,
    /// A prompt drain pass was already pending and the signals coalesced.
    Coalesced,
    /// The drain task stopped and cannot be woken.
    Closed,
}

/// Capability to wake one drain task without representing durable work in memory.
#[derive(Clone, Debug)]
pub struct WakeHandle {
    /// One-slot sender whose full state supplies coalescing.
    sender: mpsc::Sender<()>,
}

impl WakeHandle {
    /// Request one prompt bounded drain pass.
    #[must_use]
    pub fn wake(&self) -> WakeRequest {
        match self.sender.try_send(()) {
            Ok(()) => WakeRequest::Scheduled,
            Err(mpsc::error::TrySendError::Full(())) => WakeRequest::Coalesced,
            Err(mpsc::error::TrySendError::Closed(())) => WakeRequest::Closed,
        }
    }
}

/// The receiving half of a drain's wake channel, created before the task runs.
///
/// Holding a wake handle of its own keeps the channel open for as long as the
/// task lives, so a producer dropping its external handle cannot make the task's
/// wake reads report closure. Because the channel buffers one signal, a wake sent
/// after [`wake_channel`] returns but before the task is registered is retained
/// and delivered on the task's first read rather than lost.
pub struct DrainWakeReceiver {
    /// The signal stream the drain driver reads.
    receiver: mpsc::Receiver<()>,
    /// Kept only to hold the channel open for the task's lifetime.
    _guard: WakeHandle,
}

impl DrainWakeReceiver {
    /// Await the next coalesced wake, or `None` once every sender is dropped.
    pub(crate) async fn recv(&mut self) -> Option<()> {
        self.receiver.recv().await
    }
}

/// A validated doubling backoff band for a wake-triggered drain's idle sleeps.
///
/// The floor is where an idle drain's backoff starts, and the sleep after a
/// pass that found work; the ceiling is the longest sleep between passes. A
/// pass that reports when its next work falls due sleeps until then, even
/// when that is sooner than the floor. A policy whose ceiling equals its floor
/// backs off at a fixed cadence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainFallbackPolicy(BackoffSchedule);

impl DrainFallbackPolicy {
    /// Construct a fallback policy doubling from `floor` to `ceiling`.
    ///
    /// # Errors
    ///
    /// Returns [`BackoffScheduleError::ZeroFirst`] when `floor` is zero,
    /// because a zero floor would drain continuously instead of recovering a
    /// missed edge wake, and [`BackoffScheduleError::CeilingBelowFirst`] when
    /// `ceiling` is shorter than `floor`.
    pub const fn new(floor: Duration, ceiling: Duration) -> Result<DrainFallbackPolicy, BackoffScheduleError> {
        match BackoffSchedule::validated(floor, ceiling, Growth::Doubling) {
            Ok(schedule) => Ok(DrainFallbackPolicy(schedule)),
            Err(error) => Err(error),
        }
    }

    /// Return the floor: the first sleep of an idle drain that knows no due
    /// instant, and the sleep after a pass that found work.
    #[must_use]
    pub const fn floor(self) -> Duration {
        self.0.first()
    }

    /// Return the ceiling, the longest sleep between passes.
    #[must_use]
    pub const fn ceiling(self) -> Duration {
        self.0.ceiling_bound()
    }

    /// The idle sleeps as a schedule: doubling from the floor to the ceiling.
    #[must_use]
    pub const fn schedule(self) -> BackoffSchedule {
        self.0
    }
}

/// Construct one coalescing wake channel before the drain task that reads it.
///
/// The returned [`WakeHandle`] can be handed to producers immediately, before the
/// task is registered; the [`DrainWakeReceiver`] is passed to registration.
#[must_use]
pub fn wake_channel() -> (WakeHandle, DrainWakeReceiver) {
    let (sender, receiver) = mpsc::channel(1);
    let handle = WakeHandle { sender };
    let guard = handle.clone(); // clone: the receiver keeps a sender so a task's wake reads never spuriously report closure.
    (handle, DrainWakeReceiver { receiver, _guard: guard })
}
