//! Registration of long-lived resident tasks on a supervisor.

use crate::{
    driver_supervision::DriverSupervision,
    registration::TaskRegistrationError,
    resident_driver,
    resident_task::ResidentTask,
    supervisor::TaskSupervisor,
};
use std::sync::Arc;

impl TaskSupervisor {
    /// Register and start one long-lived resident task.
    ///
    /// # Errors
    ///
    /// Returns [`TaskRegistrationError`] when the task name is already reserved by this supervisor.
    pub fn register_resident(&self, task: ResidentTask) -> Result<(), TaskRegistrationError> {
        let (registration, operation) = task.into_parts();
        let health = self.reserve(&registration)?;
        let name = registration.name().clone(); // clone: the driver and supervisor registry both retain the stable task identity.
        let cancellation = self.inner.cancellation.child_token();
        let driver_health = Arc::clone(&health); // clone: driver updates are observed through the supervisor's health registry.
        let lifecycle = self.inner.lifecycle.clone(); // clone: each driver publishes into the same completion-ordered channel.
        let driver_name = name.clone(); // clone: the asynchronous driver owns its task identity.
        let restart_policy = registration.restart_policy();
        let supervision = DriverSupervision {
            name: driver_name,
            restart_policy,
            cancellation,
            health: driver_health,
            lifecycle,
        };
        let driver = self.inner.host.spawn(async move { resident_driver::run(supervision, operation).await });
        self.observe(name, &registration, health, driver);
        Ok(())
    }
}
