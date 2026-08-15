//! Wake and fallback mechanics for sequential bounded durable drains.

use crate::{
    bounded_driver,
    control::DrainExecution,
    drain_task::{DrainOperation, DrainPassOutcome},
    failure::TaskFailure,
    lifecycle_event::TaskLifecycleEvent,
    restart_policy::RestartPolicy,
    task_health::TaskHealth,
    task_name::TaskName,
    task_observer::DriverTerminal,
    wake_trigger::DrainFallbackCadence,
};
use std::{sync::Arc, time::SystemTime};
use tokio::{
    sync::{RwLock, broadcast, mpsc},
    time::{MissedTickBehavior, interval},
};
use tokio_util::sync::CancellationToken;

/// Execute wake and fallback triggers sequentially, retaining at most one wake.
pub(crate) async fn run(
    name: TaskName,
    restart_policy: RestartPolicy,
    fallback: DrainFallbackCadence,
    operation: Arc<DrainOperation>,
    mut wake: mpsc::Receiver<()>,
    cancellation: CancellationToken,
    health: Arc<RwLock<TaskHealth>>,
    lifecycle: broadcast::Sender<TaskLifecycleEvent>,
) -> DriverTerminal {
    health.write().await.running(SystemTime::now());
    let mut interval = interval(fallback.get());
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await;
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return DriverTerminal::Cancelled,
            _ = interval.tick() => {}
            signal = wake.recv() => {
                if signal.is_none() {
                    return DriverTerminal::Failed(TaskFailure::invariant_violation("drain wake channel closed while task was active"));
                }
            }
        }
        loop {
            let result = bounded_driver::execute(
                &name,
                restart_policy,
                || {
                    let execution = DrainExecution::new(cancellation.clone(), SystemTime::now()); // clone: one bounded pass owns a cooperative task-local cancellation handle.
                    operation(execution)
                },
                &cancellation,
                &health,
                &lifecycle,
            )
            .await;
            match result {
                Ok(DrainPassOutcome::Idle) => break,
                Ok(DrainPassOutcome::WorkMayRemain) => {}
                Err(terminal) => return terminal,
            }
            if cancellation.is_cancelled() {
                return DriverTerminal::Cancelled;
            }
        }
    }
}
