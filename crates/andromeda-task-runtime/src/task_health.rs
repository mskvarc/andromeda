//! Current health and execution history for one named task.

use crate::{failure::TaskFailure, task_name::TaskName};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

/// Current lifecycle state of a registered task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskHealthState {
    /// Registered but not yet executing.
    Registered,
    /// Running, idle between scheduled executions, or waiting for input.
    Running,
    /// Waiting for restart eligibility after a transient failure.
    BackingOff,
    /// A startup task completed successfully.
    Completed,
    /// The task observed cancellation and stopped.
    Cancelled,
    /// Restart policy was exhausted.
    Failed,
    /// Cooperative shutdown exceeded the task deadline.
    ForcedTermination,
}

impl TaskHealthState {
    /// Whether no further execution is active for this registration.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskHealthState::Completed | TaskHealthState::Cancelled | TaskHealthState::Failed | TaskHealthState::ForcedTermination
        )
    }
}

/// Immutable health snapshot returned to runtime observers.
#[derive(Clone, Debug)]
pub struct TaskHealthSnapshot {
    /// Registered task name.
    pub name: TaskName,
    /// Current task state.
    pub state: TaskHealthState,
    /// Time at which the current task lifecycle started.
    pub started_at: Option<SystemTime>,
    /// Last successful bounded execution time.
    pub last_success_at: Option<SystemTime>,
    /// Last failed execution time.
    pub last_failure_at: Option<SystemTime>,
    /// Last human-readable failure summary.
    pub last_failure_summary: Option<Arc<str>>,
    /// Failures since the most recent successful execution.
    pub consecutive_failures: u32,
    /// Monotonic restart deadline expressed as wall-clock observation time.
    pub backoff_deadline: Option<SystemTime>,
    /// Duration of the most recent successful bounded execution.
    pub last_success_duration: Option<Duration>,
    /// Whether cancellation has been requested for the owning scope.
    pub cancellation_requested: bool,
}

/// Mutable health record owned internally by the supervisor.
#[derive(Debug)]
pub(crate) struct TaskHealth {
    /// Latest externally observable health values.
    snapshot: TaskHealthSnapshot,
}

impl TaskHealth {
    /// Construct registered health for one named task.
    pub(crate) const fn registered(name: TaskName) -> TaskHealth {
        TaskHealth {
            snapshot: TaskHealthSnapshot {
                name,
                state: TaskHealthState::Registered,
                started_at: None,
                last_success_at: None,
                last_failure_at: None,
                last_failure_summary: None,
                consecutive_failures: 0,
                backoff_deadline: None,
                last_success_duration: None,
                cancellation_requested: false,
            },
        }
    }

    /// Record active execution.
    pub(crate) fn running(&mut self, now: SystemTime) {
        self.snapshot.state = TaskHealthState::Running;
        self.snapshot.started_at.get_or_insert(now);
        self.snapshot.backoff_deadline = None;
    }

    /// Record a successful bounded execution and reset failure state.
    pub(crate) const fn succeeded(&mut self, now: SystemTime, duration: Duration) {
        self.snapshot.state = TaskHealthState::Running;
        self.snapshot.last_success_at = Some(now);
        self.snapshot.last_success_duration = Some(duration);
        self.snapshot.consecutive_failures = 0;
        self.snapshot.backoff_deadline = None;
    }

    /// Record that a restarted resident operation reached its running state.
    pub(crate) const fn resident_running(&mut self, now: SystemTime) {
        self.snapshot.state = TaskHealthState::Running;
        self.snapshot.last_success_at = Some(now);
        self.snapshot.consecutive_failures = 0;
        self.snapshot.backoff_deadline = None;
    }

    /// Record a failure before restart policy is evaluated.
    pub(crate) fn failed_attempt(&mut self, now: SystemTime, failure: &TaskFailure) -> u32 {
        self.snapshot.last_failure_at = Some(now);
        self.snapshot.last_failure_summary = Some(failure.shared_summary());
        self.snapshot.consecutive_failures = self.snapshot.consecutive_failures.saturating_add(1);
        self.snapshot.consecutive_failures
    }

    /// Record a transient restart deadline.
    pub(crate) const fn backing_off(&mut self, deadline: SystemTime) {
        self.snapshot.state = TaskHealthState::BackingOff;
        self.snapshot.backoff_deadline = Some(deadline);
    }

    /// Record a terminal state.
    pub(crate) const fn terminal(&mut self, state: TaskHealthState) {
        self.snapshot.state = state;
        self.snapshot.backoff_deadline = None;
    }

    /// Copy the current immutable snapshot.
    pub(crate) fn snapshot(&self) -> TaskHealthSnapshot {
        self.snapshot.clone() // clone: callers receive an immutable observation independent of later updates.
    }

    /// Return whether the task has reached a terminal state.
    pub(crate) const fn is_terminal(&self) -> bool {
        self.snapshot.state.is_terminal()
    }
}
