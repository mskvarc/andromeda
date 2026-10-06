//! Completion-ordered lifecycle observations.

use crate::{criticality::TaskCriticality, failure::FailureClass, task_name::TaskName};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

/// One observable transition in a named task lifecycle.
#[derive(Clone, Debug)]
pub struct TaskLifecycleEvent {
    /// Task whose state changed.
    pub name: TaskName,
    /// Transition that occurred.
    pub kind: LifecycleEventKind,
    /// Wall-clock observation time.
    pub occurred_at: SystemTime,
}

/// Observable lifecycle transition details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleEventKind {
    /// Task execution started.
    Started,
    /// One bounded scheduled execution succeeded.
    ExecutionSucceeded {
        /// Duration of the successful execution.
        duration: Duration,
    },
    /// A transient failure entered restart backoff.
    BackingOff {
        /// Failure classification supplied by composition.
        class: FailureClass,
        /// Human-readable failure summary.
        summary: Arc<str>,
        /// Delay before the next eligible attempt.
        delay: Duration,
    },
    /// Task stopped after restart policy was exhausted.
    Failed {
        /// Failure classification supplied by composition.
        class: FailureClass,
        /// Human-readable failure summary.
        summary: Arc<str>,
        /// Operational criticality applied by supervision.
        criticality: TaskCriticality,
    },
    /// Task observed cancellation and stopped cooperatively.
    Cancelled,
    /// Cooperative cancellation exceeded the configured deadline.
    ForcedTermination,
}

impl TaskLifecycleEvent {
    /// Construct one timestamped lifecycle transition.
    pub(crate) fn new(name: TaskName, kind: LifecycleEventKind) -> TaskLifecycleEvent {
        TaskLifecycleEvent {
            name,
            kind,
            occurred_at: SystemTime::now(),
        }
    }
}
