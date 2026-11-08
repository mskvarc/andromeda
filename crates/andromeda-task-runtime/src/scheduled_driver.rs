//! Sequential execution mechanics for periodic bounded tasks.

use crate::{
    bounded_driver,
    control::ScheduledExecution,
    driver_supervision::DriverSupervision,
    failure::TaskFailure,
    failure_report::FailureReport,
    periodic_trigger::PeriodicSchedule,
    runtime_failure::RuntimeFailure,
    scheduled_task::ScheduledOperation,
    task_observer::DriverTerminal,
};
use std::{sync::Arc, time::SystemTime};
use tokio::sync::mpsc;

/// Execute periodic triggers sequentially with at most one pending signal.
pub(crate) async fn run(
    supervision: DriverSupervision,
    schedule: PeriodicSchedule,
    operation: Arc<ScheduledOperation>,
    mut run_now: mpsc::Receiver<()>,
) -> DriverTerminal {
    supervision.health.write().await.running(SystemTime::now());
    let cancellation = &supervision.cancellation;
    let mut interval = schedule.interval(&supervision.name);
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return DriverTerminal::Cancelled,
            _ = interval.tick() => {}
            signal = run_now.recv() => {
                if signal.is_none() {
                    return DriverTerminal::Failed(FailureReport::of(&TaskFailure::invariant_violation(RuntimeFailure::RunNowChannelClosed)));
                }
            }
        }
        let result = bounded_driver::execute(&supervision, || {
            let execution = ScheduledExecution::new(cancellation.clone(), SystemTime::now());
            operation(execution)
        })
        .await;
        match result {
            Ok(()) => {}
            Err(terminal) => return terminal,
        }
    }
}
