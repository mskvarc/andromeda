//! Coalescing drain wake control and validated fallback cadence.

use std::time::Duration;
use thiserror::Error;
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

/// Non-zero cadence that recovers from a missed edge wake or process restart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainFallbackCadence(Duration);

impl DrainFallbackCadence {
    /// Construct a non-zero fallback cadence.
    ///
    /// # Errors
    ///
    /// Returns [`DrainFallbackCadenceError`] when `cadence` is zero, because a zero cadence would drain continuously instead of recovering a missed edge wake.
    pub const fn new(cadence: Duration) -> Result<DrainFallbackCadence, DrainFallbackCadenceError> {
        if cadence.is_zero() {
            Err(DrainFallbackCadenceError)
        } else {
            Ok(DrainFallbackCadence(cadence))
        }
    }

    /// Return the configured cadence.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// A drain fallback cadence cannot be zero.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("drain fallback cadence must be non-zero")]
pub struct DrainFallbackCadenceError;

/// Construct one coalescing wake channel.
pub(crate) fn wake_channel() -> (WakeHandle, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel(1);
    (WakeHandle { sender }, receiver)
}
