//! Terminal task observation, lifecycle reporting and criticality escalation.

use crate::{
    criticality::TaskCriticality,
    failure::TaskFailure,
    lifecycle_event::{LifecycleEventKind, TaskLifecycleEvent},
    task_health::{TaskHealth, TaskHealthState},
    task_name::TaskName,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};
use tokio::{
    sync::{Notify, RwLock, broadcast},
    task::JoinError,
};
use tokio_util::sync::CancellationToken;

/// Terminal value returned by an execution driver to its observer.
pub(crate) enum DriverTerminal {
    /// The driver observed task-scope cancellation.
    Cancelled,
    /// Restart policy was exhausted for this classified failure.
    Failed(TaskFailure),
}

/// Attribute one driver's terminal result and apply its criticality.
pub(crate) async fn observe_terminal(
    scope_cancellation: &CancellationToken,
    process_cancellation: &CancellationToken,
    lifecycle: &broadcast::Sender<TaskLifecycleEvent>,
    completion: &Notify,
    name: TaskName,
    criticality: TaskCriticality,
    health: Arc<RwLock<TaskHealth>>,
    forced: Arc<AtomicBool>,
    terminal: DriverTerminal,
) {
    let kind = if forced.load(Ordering::Acquire) {
        health.write().await.terminal(TaskHealthState::ForcedTermination);
        LifecycleEventKind::ForcedTermination
    } else {
        match terminal {
            DriverTerminal::Cancelled => {
                health.write().await.terminal(TaskHealthState::Cancelled);
                LifecycleEventKind::Cancelled
            }
            DriverTerminal::Failed(failure) => {
                let mut record = health.write().await;
                if record.snapshot().last_failure_summary.as_deref() != Some(failure.summary()) {
                    record.failed_attempt(SystemTime::now(), &failure);
                }
                record.terminal(TaskHealthState::Failed);
                drop(record);
                apply_criticality(scope_cancellation, process_cancellation, criticality);
                LifecycleEventKind::Failed {
                    class: failure.class(),
                    summary: failure.shared_summary(),
                    criticality,
                }
            }
        }
    };
    emit(lifecycle, TaskLifecycleEvent::new(name, kind));
    completion.notify_waiters();
}

/// Apply exhausted criticality without conflating it with failure classification.
pub(crate) fn apply_criticality(scope_cancellation: &CancellationToken, process_cancellation: &CancellationToken, criticality: TaskCriticality) {
    match criticality {
        TaskCriticality::Observed | TaskCriticality::TenantDegrading => {}
        TaskCriticality::TenantFatal => scope_cancellation.cancel(),
        TaskCriticality::ProcessFatal => process_cancellation.cancel(),
    }
}

/// Convert a driver panic or external abort into an invariant failure.
pub(crate) fn join_failure(error: &JoinError) -> TaskFailure {
    TaskFailure::invariant_violation(format!("task execution stopped outside its runtime contract: {error}"))
}

/// Publish a lifecycle event and tolerate the absence of diagnostic subscribers.
pub(crate) fn emit(sender: &broadcast::Sender<TaskLifecycleEvent>, event: TaskLifecycleEvent) {
    if sender.send(event).is_err() {
        tracing::trace!("task lifecycle event had no active subscriber");
    }
}
