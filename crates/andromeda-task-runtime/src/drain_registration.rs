//! Registration of wake-triggered durable drains on a supervisor.

use crate::{
    drain_driver,
    drain_task::DrainTask,
    driver_supervision::DriverSupervision,
    registration::TaskRegistrationError,
    supervisor::TaskSupervisor,
    wake_trigger::DrainWakeReceiver,
};
use std::sync::Arc;

impl TaskSupervisor {
    /// Register and start one wake-triggered durable drain, reading its wakes from
    /// a receiver created before the task.
    ///
    /// The wake channel is built by [`wake_channel`](crate::wake_trigger::wake_channel)
    /// before registration so producers can hold the wake handle while the task
    /// is still being assembled; a wake sent in that window is retained and
    /// delivered on the task's first read.
    ///
    /// # Errors
    ///
    /// Returns [`TaskRegistrationError`] when the task name is already reserved by
    /// this supervisor.
    pub fn register_drain(&self, task: DrainTask, wake: DrainWakeReceiver) -> Result<(), TaskRegistrationError> {
        let (registration, fallback, operation) = task.into_parts();
        let health = self.reserve(&registration)?;
        let name = registration.name().clone(); // clone: the driver and supervisor registry both retain the stable task identity.
        let cancellation = self.inner.cancellation.child_token();
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
        let driver = self
            .inner
            .host
            .spawn(async move { drain_driver::run(supervision, fallback, operation, wake).await });
        self.observe(name, &registration, health, driver);
        Ok(())
    }
}
