//! Complete task registration metadata.

use crate::{criticality::TaskCriticality, restart_policy::RestartPolicy, shutdown_policy::ShutdownPolicy, task_name::TaskName};
use thiserror::Error;

/// Metadata fixed before a task becomes visible to supervision.
#[derive(Clone, Debug)]
pub struct TaskRegistration {
    /// Stable identity unique within one supervisor.
    name: TaskName,
    /// Operational effect after restart exhaustion.
    criticality: TaskCriticality,
    /// Retry and backoff behavior.
    restart_policy: RestartPolicy,
    /// Cooperative stop deadline.
    shutdown_policy: ShutdownPolicy,
}

impl TaskRegistration {
    /// Construct complete task metadata.
    pub const fn new(name: TaskName, criticality: TaskCriticality, restart_policy: RestartPolicy, shutdown_policy: ShutdownPolicy) -> TaskRegistration {
        TaskRegistration {
            name,
            criticality,
            restart_policy,
            shutdown_policy,
        }
    }

    /// Return the task name.
    pub const fn name(&self) -> &TaskName {
        &self.name
    }

    /// Return the task criticality.
    pub const fn criticality(&self) -> TaskCriticality {
        self.criticality
    }

    /// Return the restart policy.
    pub const fn restart_policy(&self) -> RestartPolicy {
        self.restart_policy
    }

    /// Return the shutdown policy.
    pub const fn shutdown_policy(&self) -> ShutdownPolicy {
        self.shutdown_policy
    }
}

/// A task name was already registered in the supervisor.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("task `{name}` is already registered")]
pub struct TaskRegistrationError {
    /// The duplicate task name.
    pub name: TaskName,
}
