//! Failure, health and restart mechanics shared by bounded execution forms.

use crate::{
    driver_supervision::DriverSupervision,
    failure_report::FailureReport,
    lifecycle_event::{LifecycleEventKind, TaskLifecycleEvent},
    task_observer::{DriverTerminal, emit},
};
use std::{future::Future, time::SystemTime};
use tokio::time::{Instant, sleep};

/// Execute one bounded trigger, retrying only failures admitted by restart policy.
pub(crate) async fn execute<O, F, Fut>(supervision: &DriverSupervision, operation: F) -> Result<O, DriverTerminal>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<O, FailureReport>>,
{
    let DriverSupervision {
        name,
        restart_policy,
        cancellation,
        health,
        lifecycle,
    } = supervision;
    loop {
        let started = Instant::now();
        health.write().await.running(SystemTime::now());
        emit(lifecycle, TaskLifecycleEvent::new(name.clone(), LifecycleEventKind::Started)); // clone: the health registry retains the canonical task name.
        match operation().await {
            Ok(outcome) => {
                let duration = started.elapsed();
                health.write().await.succeeded(SystemTime::now(), duration);
                emit(
                    lifecycle,
                    TaskLifecycleEvent::new(name.clone(), LifecycleEventKind::ExecutionSucceeded { duration }), // clone: subsequent executions retain the task name.
                );
                return Ok(outcome);
            }
            Err(_failure) if cancellation.is_cancelled() => return Err(DriverTerminal::Cancelled),
            Err(failure) => {
                let count = health.write().await.failed_attempt(SystemTime::now(), &failure);
                let Some(delay) = restart_policy.delay(failure.class(), count) else {
                    return Err(DriverTerminal::Failed(failure));
                };
                health.write().await.backing_off(SystemTime::now() + delay);
                emit(
                    lifecycle,
                    TaskLifecycleEvent::new(
                        name.clone(), // clone: restart preserves the same task identity.
                        LifecycleEventKind::BackingOff {
                            class: failure.class(),
                            summary: failure.shared_summary(),
                            delay,
                        },
                    ),
                );
                tokio::select! {
                    () = cancellation.cancelled() => return Err(DriverTerminal::Cancelled),
                    () = sleep(delay) => {}
                }
            }
        }
    }
}
