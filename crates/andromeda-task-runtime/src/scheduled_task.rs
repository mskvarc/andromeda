//! Scheduled bounded task definition.

use crate::{control::ScheduledExecution, failure::TaskFailure, periodic_trigger::PeriodicSchedule, registration::TaskRegistration};
use std::{future::Future, pin::Pin, sync::Arc};

/// Type-erased bounded operation invoked by scheduled execution mechanics.
pub(crate) type ScheduledOperation = dyn Fn(ScheduledExecution) -> Pin<Box<dyn Future<Output = Result<(), TaskFailure>> + Send + 'static>> + Send + Sync;

/// Complete scheduled task definition accepted by a supervisor.
pub struct ScheduledTask {
    /// Complete metadata fixed before execution.
    registration: TaskRegistration,
    /// Periodic trigger definition.
    schedule: PeriodicSchedule,
    /// Bounded operation supplied by composition.
    operation: Arc<ScheduledOperation>,
}

impl ScheduledTask {
    /// Adapt a bounded operation into one scheduled task definition.
    pub fn new<F, Fut>(registration: TaskRegistration, schedule: PeriodicSchedule, operation: F) -> ScheduledTask
    where
        F: Fn(ScheduledExecution) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), TaskFailure>> + Send + 'static,
    {
        ScheduledTask {
            registration,
            schedule,
            operation: Arc::new(move |execution| Box::pin(operation(execution))),
        }
    }

    /// Decompose the task for supervisor registration.
    pub(crate) fn into_parts(self) -> (TaskRegistration, PeriodicSchedule, Arc<ScheduledOperation>) {
        (self.registration, self.schedule, self.operation)
    }
}
