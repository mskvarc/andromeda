//! What the runtime records about one failed task attempt.

use crate::failure::{FailureClass, TaskFailure};
use std::{fmt::Display, sync::Arc};

/// The record of one failed attempt, as health snapshots and lifecycle events
/// carry it.
///
/// The runtime supervises tasks whose failures have unrelated types, and all it
/// does with a failure beyond restart classification is report it to an
/// operator. So a [`TaskFailure`] is rendered here, once, at the boundary where
/// a task's typed error meets the runtime, and only this record travels on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FailureReport {
    /// Runtime classification supplied by composition.
    class: FailureClass,
    /// The rendered cause, shared by health and lifecycle reporting.
    summary: Arc<str>,
}

impl FailureReport {
    /// Record a classified failure, rendering its cause.
    pub(crate) fn of<E: Display>(failure: &TaskFailure<E>) -> FailureReport {
        FailureReport {
            class: failure.class(),
            summary: Arc::from(failure.cause().to_string()),
        }
    }

    /// Return the runtime failure class.
    pub(crate) const fn class(&self) -> FailureClass {
        self.class
    }

    /// Return the rendered cause.
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }

    /// Share the rendered cause with lifecycle and health observers.
    pub(crate) fn shared_summary(&self) -> Arc<str> {
        Arc::clone(&self.summary) // clone: health and lifecycle events retain the same immutable summary.
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        failure::{FailureClass, TaskFailure},
        failure_report::FailureReport,
        runtime_failure::RuntimeFailure,
    };

    #[test]
    fn a_report_keeps_the_class_and_renders_the_typed_cause() {
        let report = FailureReport::of(&TaskFailure::invariant_violation(RuntimeFailure::DrainWakeChannelClosed));
        assert_eq!(report.class(), FailureClass::InvariantViolation);
        assert_eq!(report.summary(), "drain wake channel closed while task was active");
    }
}
