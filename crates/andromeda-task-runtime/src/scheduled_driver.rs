//! Sequential execution mechanics for periodic bounded tasks.

use crate::{
    control::ScheduledExecution,
    failure::TaskFailure,
    lifecycle_event::{LifecycleEventKind, TaskLifecycleEvent},
    periodic_trigger::PeriodicSchedule,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledOperation,
    task_health::TaskHealth,
    task_name::TaskName,
    task_observer::{DriverTerminal, emit},
};
use std::{sync::Arc, time::SystemTime};
use tokio::{
    sync::{RwLock, broadcast, mpsc},
    time::{Instant, sleep},
};
use tokio_util::sync::CancellationToken;

/// Execute periodic triggers sequentially with at most one pending signal.
pub(crate) async fn run(
    name: TaskName,
    restart_policy: RestartPolicy,
    schedule: PeriodicSchedule,
    operation: Arc<ScheduledOperation>,
    mut run_now: mpsc::Receiver<()>,
    cancellation: CancellationToken,
    health: Arc<RwLock<TaskHealth>>,
    lifecycle: broadcast::Sender<TaskLifecycleEvent>,
) -> DriverTerminal {
    health.write().await.running(SystemTime::now());
    let mut interval = schedule.interval(&name);
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return DriverTerminal::Cancelled,
            _ = interval.tick() => {}
            signal = run_now.recv() => {
                if signal.is_none() {
                    return DriverTerminal::Failed(TaskFailure::invariant_violation("scheduled run-now channel closed while task was active"));
                }
            }
        }
        match execute(&name, restart_policy, &operation, &cancellation, &health, &lifecycle).await {
            Ok(()) => {}
            Err(terminal) => return terminal,
        }
    }
}

/// Execute one trigger, retrying only failures admitted by restart policy.
async fn execute(
    name: &TaskName,
    restart_policy: RestartPolicy,
    operation: &Arc<ScheduledOperation>,
    cancellation: &CancellationToken,
    health: &RwLock<TaskHealth>,
    lifecycle: &broadcast::Sender<TaskLifecycleEvent>,
) -> Result<(), DriverTerminal> {
    loop {
        let started = Instant::now();
        health.write().await.running(SystemTime::now());
        emit(lifecycle, TaskLifecycleEvent::new(name.clone(), LifecycleEventKind::Started)); // clone: the health registry retains the canonical task name.
        let execution = ScheduledExecution::new(cancellation.clone(), SystemTime::now()); // clone: the operation receives a cooperative handle to this task's cancellation state.
        match operation(execution).await {
            Ok(()) => {
                let duration = started.elapsed();
                health.write().await.succeeded(SystemTime::now(), duration);
                emit(
                    lifecycle,
                    TaskLifecycleEvent::new(name.clone(), LifecycleEventKind::ExecutionSucceeded { duration }), // clone: subsequent executions retain the task name.
                );
                return Ok(());
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
