//! Wake and backoff mechanics for sequential bounded durable drains.

use crate::{
    bounded_driver,
    control::{DrainCause, DrainExecution},
    drain_backoff::DrainBackoff,
    drain_task::{DrainOperation, DrainPassOutcome},
    driver_supervision::DriverSupervision,
    failure::TaskFailure,
    failure_report::FailureReport,
    pass_wakes::PassWakes,
    runtime_failure::RuntimeFailure,
    task_observer::DriverTerminal,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver},
};
use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::time::sleep;

/// Drive one durable drain: run a bounded claim, then sleep for the backoff the
/// pass earned, waking early on an edge signal, until cancellation.
pub(crate) async fn run(
    supervision: DriverSupervision,
    policy: DrainFallbackPolicy,
    operation: Arc<DrainOperation>,
    mut wake: DrainWakeReceiver,
) -> DriverTerminal {
    supervision.health.write().await.running(SystemTime::now());
    let cancellation = &supervision.cancellation;
    let mut backoff = DrainBackoff::new(policy);
    // The first pass is deferred by the floor rather than run eagerly on start:
    // a level-triggered drain waits for a wake or the fallback so a freshly loaded
    // tenant does not fire every drain the instant it is cached.
    let mut next = NextSleep {
        delay: backoff.record_idle(),
        expiry: DrainCause::Fallback,
    };
    loop {
        let cause = tokio::select! {
            biased;
            () = cancellation.cancelled() => return DriverTerminal::Cancelled,
            signal = wake.recv() => {
                if signal.is_none() {
                    return wake_channel_closed();
                }
                backoff.record_wake();
                DrainCause::Wake
            }
            () = sleep(next.delay) => next.expiry,
        };
        next = match drain_until_settled(&supervision, &operation, &mut backoff, cause, &mut wake).await {
            Ok(next) => next,
            Err(terminal) => return terminal,
        };
    }
}

/// The sleep before the next cycle, and what the pass that ends it is.
struct NextSleep {
    /// How long the driver sleeps unless a wake comes first.
    delay: Duration,
    /// The cause of the pass that runs when the sleep ends.
    expiry: DrainCause,
}

/// Run bounded passes until one settles, returning the sleep before the next cycle.
///
/// Repeats immediately while a pass reports work may remain; a pass that found
/// work resets the backoff to the floor, a bare idle pass backs off, and a pass
/// with a known next-due time sleeps until then, at most the ceiling. A sleep
/// that reaches the due instant a pass reported ends in a [`DrainCause::Due`]
/// pass; every other sleep ends in a [`DrainCause::Fallback`] pass. The first
/// pass carries `cause`; every repeat is a [`DrainCause::Continuation`].
///
/// Each pass is handed the wakes that arrive while it runs, and a wake it
/// returns without taking in is queued again on `wake`, so it starts a pass
/// as it would have had it arrived between passes.
async fn drain_until_settled(
    supervision: &DriverSupervision,
    operation: &Arc<DrainOperation>,
    backoff: &mut DrainBackoff,
    cause: DrainCause,
    wake: &mut DrainWakeReceiver,
) -> Result<NextSleep, DriverTerminal> {
    let cancellation = &supervision.cancellation;
    let mut did_work = false;
    let mut cause = cause;
    loop {
        let pass_cause = cause;
        let wakes = PassWakes::default();
        let pass = bounded_driver::execute(supervision, || {
            let execution = DrainExecution::new(cancellation.clone(), SystemTime::now(), pass_cause, wakes.clone());
            operation(execution)
        });
        let result = handing_wakes(pass, wake, &wakes, backoff).await?;
        wakes.hand_back(wake);
        cause = DrainCause::Continuation;
        match result {
            Ok(DrainPassOutcome::WorkMayRemain) => did_work = true,
            Ok(DrainPassOutcome::Idle) => {
                let delay = if did_work { backoff.record_worked() } else { backoff.record_idle() };
                return Ok(NextSleep {
                    delay,
                    expiry: DrainCause::Fallback,
                });
            }
            Ok(DrainPassOutcome::IdleUntil(due)) => {
                let now = SystemTime::now();
                let delay = backoff.record_idle_until(due, now);
                // A due instant beyond the ceiling is not reached by this sleep.
                let expiry = if now.checked_add(delay).is_some_and(|end| end >= due) {
                    DrainCause::Due
                } else {
                    DrainCause::Fallback
                };
                return Ok(NextSleep { delay, expiry });
            }
            Ok(DrainPassOutcome::RestUntil(until)) => {
                return Ok(NextSleep {
                    delay: backoff.record_idle_until(until, SystemTime::now()),
                    expiry: DrainCause::Fallback,
                });
            }
            Err(terminal) => return Err(terminal),
        }
        if cancellation.is_cancelled() {
            return Err(DriverTerminal::Cancelled);
        }
    }
}

/// Run `pass` to its end, handing it each wake that arrives on `wake`
/// meanwhile through `wakes`.
async fn handing_wakes<O>(
    pass: impl Future<Output = O>,
    wake: &mut DrainWakeReceiver,
    wakes: &PassWakes,
    backoff: &mut DrainBackoff,
) -> Result<O, DriverTerminal> {
    let mut pass = pin!(pass);
    loop {
        tokio::select! {
            biased;
            result = &mut pass => return Ok(result),
            signal = wake.recv() => match signal {
                Some(()) => {
                    backoff.record_wake();
                    wakes.raise();
                }
                None => return Err(wake_channel_closed()),
            },
        }
    }
}

/// The terminal of a drain whose wake channel closed, which its own sender
/// rules out.
fn wake_channel_closed() -> DriverTerminal {
    DriverTerminal::Failed(FailureReport::of(&TaskFailure::invariant_violation(RuntimeFailure::DrainWakeChannelClosed)))
}
