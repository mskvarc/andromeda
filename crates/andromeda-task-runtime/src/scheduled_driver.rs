//! Sequential execution mechanics for periodic bounded tasks.

use crate::{
    bounded_driver,
    control::ScheduledExecution,
    failure::TaskFailure,
    lifecycle_event::TaskLifecycleEvent,
    periodic_trigger::PeriodicSchedule,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledOperation,
    task_health::TaskHealth,
    task_name::TaskName,
    task_observer::DriverTerminal,
};
use std::{sync::Arc, time::SystemTime};
use tokio::sync::{RwLock, broadcast, mpsc};
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
        let result = bounded_driver::execute(
            &name,
            restart_policy,
            || {
                let execution = ScheduledExecution::new(cancellation.clone(), SystemTime::now()); // clone: the operation receives a cooperative handle to this task's cancellation state.
                operation(execution)
            },
            &cancellation,
            &health,
            &lifecycle,
        )
        .await;
        match result {
            Ok(()) => {}
            Err(terminal) => return terminal,
        }
    }
}
