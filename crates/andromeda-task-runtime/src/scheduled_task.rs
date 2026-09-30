//! Scheduled bounded task definition.

use crate::{
    control::ScheduledExecution,
    failure::TaskFailure,
    failure_report::FailureReport,
    periodic_trigger::PeriodicSchedule,
    registration::TaskRegistration,
};
use std::{error::Error, future::Future, pin::Pin, sync::Arc};

/// Type-erased bounded operation invoked by scheduled execution mechanics.
///
/// The operation's typed failure is rendered into a [`FailureReport`] where it
/// is adapted, so drivers of unrelated tasks share one operation shape.
pub(crate) type ScheduledOperation = dyn Fn(ScheduledExecution) -> Pin<Box<dyn Future<Output = Result<(), FailureReport>> + Send + 'static>> + Send + Sync;

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
    pub fn new<F, Fut, E>(registration: TaskRegistration, schedule: PeriodicSchedule, operation: F) -> ScheduledTask
    where
        F: Fn(ScheduledExecution) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), TaskFailure<E>>> + Send + 'static,
        E: Error,
    {
        ScheduledTask {
            registration,
            schedule,
            operation: Arc::new(move |execution| {
                let attempt = operation(execution);
                Box::pin(async move { attempt.await.map_err(|failure| FailureReport::of(&failure)) })
            }),
        }
    }

    /// Decompose the task for supervisor registration.
    pub(crate) fn into_parts(self) -> (TaskRegistration, PeriodicSchedule, Arc<ScheduledOperation>) {
        (self.registration, self.schedule, self.operation)
    }
}
