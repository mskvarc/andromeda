//! Restart mechanics for long-lived resident tasks.

use crate::{
    control::{ResidentExecution, ResidentReadiness},
    failure::TaskFailure,
    lifecycle_event::{LifecycleEventKind, TaskLifecycleEvent},
    resident_task::ResidentOperation,
    restart_policy::RestartPolicy,
    task_health::TaskHealth,
    task_name::TaskName,
    task_observer::{DriverTerminal, emit},
};
use std::{sync::Arc, time::SystemTime};
use tokio::{
    sync::{RwLock, broadcast},
    time::sleep,
};
use tokio_util::sync::CancellationToken;

/// Execute a resident operation, treating unrequested completion as an unexpected transient exit.
pub(crate) async fn run(
    name: TaskName,
    restart_policy: RestartPolicy,
    operation: Arc<ResidentOperation>,
    cancellation: CancellationToken,
    health: Arc<RwLock<TaskHealth>>,
    lifecycle: broadcast::Sender<TaskLifecycleEvent>,
) -> DriverTerminal {
    loop {
        health.write().await.running(SystemTime::now());
        emit(&lifecycle, TaskLifecycleEvent::new(name.clone(), LifecycleEventKind::Started)); // clone: restart attempts retain one stable task identity.
        let readiness = Arc::new(ResidentReadiness::default());
        let execution = ResidentExecution::new(cancellation.clone(), Arc::clone(&readiness)); // clone: the resident operation and driver coordinate one readiness transition.
        let attempt = operation(execution);
        tokio::pin!(attempt);
        let (result, running_recorded) = tokio::select! {
            biased;
            () = readiness.wait() => {
                health.write().await.resident_running(SystemTime::now());
                (attempt.await, true)
            }
            result = &mut attempt => (result, false),
        };
        if !running_recorded && readiness.is_running() {
            health.write().await.resident_running(SystemTime::now());
        }
        if cancellation.is_cancelled() {
            return DriverTerminal::Cancelled;
        }
        let failure = match result {
            Ok(()) => TaskFailure::transient("resident task completed without cancellation"),
            Err(failure) => failure,
        };
        let count = health.write().await.failed_attempt(SystemTime::now(), &failure);
        let Some(delay) = restart_policy.delay(failure.class(), count) else {
            return DriverTerminal::Failed(failure);
        };
        health.write().await.backing_off(SystemTime::now() + delay);
        emit(
            &lifecycle,
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
            () = cancellation.cancelled() => return DriverTerminal::Cancelled,
            () = sleep(delay) => {}
        }
    }
}
