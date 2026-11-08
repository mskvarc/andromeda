//! Registration of periodic bounded tasks on a supervisor.

use crate::{
    control::{RunNowHandle, run_now_channel},
    driver_supervision::DriverSupervision,
    registration::TaskRegistrationError,
    scheduled_driver,
    scheduled_task::ScheduledTask,
    supervisor::TaskSupervisor,
};
use std::sync::Arc;

impl TaskSupervisor {
    /// Register and start one periodic bounded task.
    ///
    /// # Errors
    ///
    /// Returns [`TaskRegistrationError`] when the task name is already reserved by this supervisor.
    pub fn register_scheduled(&self, task: ScheduledTask) -> Result<RunNowHandle, TaskRegistrationError> {
        let (registration, schedule, operation) = task.into_parts();
        // Spread this scope's tasks: the seed distinguishes them from the same
        // task in other scopes, and a scope with a default fills in a spread for
        // any task that chose none. The process scope leaves both zero, so its
        // singleton tasks keep their exact cadence.
        let schedule = schedule.seeded(self.inner.scope_seed).with_default_jitter(self.inner.scope_default_jitter);
        let health = self.reserve(&registration)?;
        let name = registration.name().clone(); // clone: the driver and supervisor registry both retain the stable task identity.
        let cancellation = self.inner.cancellation.child_token();
        let (run_now, receiver) = run_now_channel();
        // Keeping a sender in the driver prevents a dropped external handle from closing the select branch.
        let run_now_guard = run_now.clone();
        let driver_health = Arc::clone(&health);
        let lifecycle = self.inner.lifecycle.clone();
        let driver_name = name.clone(); // clone: the asynchronous driver owns its task identity.
        let restart_policy = registration.restart_policy();
        let supervision = DriverSupervision {
            name: driver_name,
            restart_policy,
            cancellation,
            health: driver_health,
            lifecycle,
        };
        let driver = self.inner.host.spawn(async move {
            let _run_now_guard = run_now_guard;
            scheduled_driver::run(supervision, schedule, operation, receiver).await
        });
        self.observe(name, &registration, health, driver);
        Ok(run_now)
    }
}
