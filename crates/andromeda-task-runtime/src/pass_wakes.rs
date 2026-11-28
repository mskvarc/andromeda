//! The wakes that reach a drain while one of its passes runs.
//!
//! A producer wakes a drain once it has enqueued work. While a pass runs, the
//! driver hands each wake to that pass, so a pass still waiting on slow work
//! it started can take up what was enqueued meanwhile instead of leaving it
//! to the pass after it. A wake the pass has not taken in when it returns
//! starts the next pass, as a wake between passes does.

use crate::wake_trigger::DrainWakeReceiver;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

/// The wakes that reached a drain since its pass began, for the pass to take
/// in.
///
/// One pass takes wakes in, through one [`PassWakes::next`] at a time.
#[derive(Clone, Debug, Default)]
pub struct PassWakes(Arc<PassWakeSignal>);

/// The state the driver and the pass share.
#[derive(Debug, Default)]
struct PassWakeSignal {
    /// Whether a wake arrived that the pass has not taken in.
    pending: AtomicBool,
    /// Rouses a pass waiting in [`PassWakes::next`].
    arrived: Notify,
}

impl PassWakes {
    /// Wait until a wake has arrived since the pass began, or since the last
    /// one this returned, and take it in.
    ///
    /// Wakes that arrive before the pass takes one in coalesce. A producer
    /// wakes the drain after its work is enqueued, so work the pass starts
    /// once this returns sees everything enqueued before the wakes it took
    /// in.
    pub async fn next(&self) {
        loop {
            let arrived = self.0.arrived.notified();
            if self.0.pending.swap(false, Ordering::AcqRel) {
                return;
            }
            arrived.await;
        }
    }

    /// Hand one wake to the pass.
    pub(crate) fn raise(&self) {
        self.0.pending.store(true, Ordering::Release);
        self.0.arrived.notify_one();
    }

    /// Requeue on `receiver` a wake the pass returned without taking in, so
    /// that it starts the next pass.
    pub(crate) fn hand_back(&self, receiver: &DrainWakeReceiver) {
        if self.0.pending.swap(false, Ordering::AcqRel) {
            receiver.requeue();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        pass_wakes::PassWakes,
        wake_trigger::{WakeRequest, wake_channel},
    };
    use std::time::Duration;
    use tokio::{task::yield_now, time::timeout};

    /// Longer than any wait a raised wake leaves: the clock is paused, so it
    /// passes only once nothing else can run.
    const UNANSWERED: Duration = Duration::from_secs(60);

    #[tokio::test(start_paused = true)]
    async fn a_pass_with_no_wake_waits() {
        assert!(timeout(UNANSWERED, PassWakes::default().next()).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn wakes_raised_before_the_pass_takes_one_in_coalesce_into_one() {
        let wakes = PassWakes::default();
        wakes.raise();
        wakes.raise();
        assert!(timeout(UNANSWERED, wakes.next()).await.is_ok());
        assert!(timeout(UNANSWERED, wakes.next()).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn a_wake_raised_while_the_pass_waits_ends_its_wait() {
        let wakes = PassWakes::default();
        let raised = wakes.clone();
        let waiting = tokio::spawn(async move { timeout(UNANSWERED, wakes.next()).await });
        yield_now().await;
        raised.raise();
        assert!(matches!(waiting.await, Ok(Ok(()))));
    }

    #[test]
    fn a_wake_left_untaken_is_requeued_for_the_next_pass_and_a_taken_one_is_not() {
        let (handle, receiver) = wake_channel();
        let wakes = PassWakes::default();
        wakes.hand_back(&receiver);
        assert_eq!(handle.wake(), WakeRequest::Scheduled, "nothing was requeued");
        let (handle, receiver) = wake_channel();
        wakes.raise();
        wakes.hand_back(&receiver);
        assert_eq!(handle.wake(), WakeRequest::Coalesced, "the untaken wake waits on the channel");
    }
}
