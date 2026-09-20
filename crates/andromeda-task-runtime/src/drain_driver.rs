//! Wake and backoff mechanics for sequential bounded durable drains.

use crate::{
    bounded_driver,
    control::DrainExecution,
    drain_backoff::DrainBackoff,
    drain_task::{DrainOperation, DrainPassOutcome},
    driver_supervision::DriverSupervision,
    failure::TaskFailure,
    task_observer::DriverTerminal,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver},
};
use std::{
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
    let mut delay = backoff.record_idle();
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return DriverTerminal::Cancelled,
            signal = wake.recv() => {
                if signal.is_none() {
                    return DriverTerminal::Failed(TaskFailure::invariant_violation("drain wake channel closed while task was active"));
                }
                backoff.record_wake();
            }
            () = sleep(delay) => {}
        }
        delay = match drain_until_settled(&supervision, &operation, &mut backoff).await {
            Ok(next) => next,
            Err(terminal) => return terminal,
        };
    }
}

/// Run bounded passes until one settles, returning the sleep before the next cycle.
///
/// Repeats immediately while a pass reports work may remain; a pass that found
/// work resets the backoff to the floor, a bare idle pass backs off, and a pass
/// with a known next-due time sleeps until then within the band.
async fn drain_until_settled(supervision: &DriverSupervision, operation: &Arc<DrainOperation>, backoff: &mut DrainBackoff) -> Result<Duration, DriverTerminal> {
    let cancellation = &supervision.cancellation;
    let mut did_work = false;
    loop {
        let result = bounded_driver::execute(supervision, || {
            let execution = DrainExecution::new(cancellation.clone(), SystemTime::now()); // clone: one bounded pass owns a cooperative task-local cancellation handle.
            operation(execution)
        })
        .await;
        match result {
            Ok(DrainPassOutcome::WorkMayRemain) => did_work = true,
            Ok(DrainPassOutcome::Idle) => return Ok(if did_work { backoff.record_worked() } else { backoff.record_idle() }),
            Ok(DrainPassOutcome::IdleUntil(due)) => return Ok(backoff.record_idle_until(due, SystemTime::now())),
            Err(terminal) => return Err(terminal),
        }
        if cancellation.is_cancelled() {
            return Err(DriverTerminal::Cancelled);
        }
    }
}
