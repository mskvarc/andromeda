//! Wake and fallback mechanics for sequential bounded durable drains.

use crate::{
    bounded_driver,
    control::DrainExecution,
    drain_task::{DrainOperation, DrainPassOutcome},
    driver_supervision::DriverSupervision,
    failure::TaskFailure,
    task_observer::DriverTerminal,
    wake_trigger::DrainFallbackCadence,
};
use std::{sync::Arc, time::SystemTime};
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};

/// Execute wake and fallback triggers sequentially, retaining at most one wake.
pub(crate) async fn run(
    supervision: DriverSupervision,
    fallback: DrainFallbackCadence,
    operation: Arc<DrainOperation>,
    mut wake: mpsc::Receiver<()>,
) -> DriverTerminal {
    supervision.health.write().await.running(SystemTime::now());
    let cancellation = &supervision.cancellation;
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
            let result = bounded_driver::execute(&supervision, || {
                let execution = DrainExecution::new(cancellation.clone(), SystemTime::now()); // clone: one bounded pass owns a cooperative task-local cancellation handle.
                operation(execution)
            })
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
