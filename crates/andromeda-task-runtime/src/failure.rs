//! Failure information classified by broker composition.

use std::sync::Arc;
use thiserror::Error;

/// Runtime action class assigned to a typed policy or operational failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    /// Re-execution can succeed without configuration or code changes.
    Transient,
    /// Re-execution requires an external state change.
    Permanent,
    /// The failure contradicts an internal correctness invariant.
    InvariantViolation,
}

/// Classified failure accepted by the task runtime boundary.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{summary}")]
pub struct TaskFailure {
    /// Runtime classification supplied by composition.
    class: FailureClass,
    /// Immutable diagnostic shared by health and lifecycle reporting.
    summary: Arc<str>,
}

impl TaskFailure {
    /// Construct a transient failure with a human-readable summary.
    pub fn transient(summary: impl Into<Arc<str>>) -> TaskFailure {
        TaskFailure {
            class: FailureClass::Transient,
            summary: summary.into(),
        }
    }

    /// Construct a permanent failure with a human-readable summary.
    pub fn permanent(summary: impl Into<Arc<str>>) -> TaskFailure {
        TaskFailure {
            class: FailureClass::Permanent,
            summary: summary.into(),
        }
    }

    /// Construct an invariant-violation failure with a human-readable summary.
    pub fn invariant_violation(summary: impl Into<Arc<str>>) -> TaskFailure {
        TaskFailure {
            class: FailureClass::InvariantViolation,
            summary: summary.into(),
        }
    }

    /// Return the runtime failure class.
    pub const fn class(&self) -> FailureClass {
        self.class
    }

    /// Return the human-readable summary.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Share the failure summary with lifecycle and health observers.
    pub fn shared_summary(&self) -> Arc<str> {
        Arc::clone(&self.summary) // clone: health and lifecycle events retain the same immutable summary.
    }
}
