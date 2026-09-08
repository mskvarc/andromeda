//! What the supervisor hands one execution driver to run a named task under.

use crate::{lifecycle_event::TaskLifecycleEvent, restart_policy::RestartPolicy, task_health::TaskHealth, task_name::TaskName};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio_util::sync::CancellationToken;

/// The supervision one execution driver runs a named task under.
///
/// The five parts travel together because every driver's restart loop uses all
/// of them at once: a classified failure is recorded against the health record,
/// weighed by the restart policy, published on the lifecycle channel under the
/// task's registered name, and abandoned the moment its scope is cancelled.
pub(crate) struct DriverSupervision {
    /// Registered identity every lifecycle event is published under.
    pub(crate) name: TaskName,
    /// Restart decision applied to each classified failure.
    pub(crate) restart_policy: RestartPolicy,
    /// Task-scope cancellation the driver stops cooperatively on.
    pub(crate) cancellation: CancellationToken,
    /// Health record the supervisor observes this task through.
    pub(crate) health: Arc<RwLock<TaskHealth>>,
    /// Completion-ordered channel every lifecycle transition is published on.
    pub(crate) lifecycle: broadcast::Sender<TaskLifecycleEvent>,
}
