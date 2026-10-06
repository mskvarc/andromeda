//! Failure information classified by broker composition.

use strum::IntoStaticStr;
use thiserror::Error;

/// Runtime action class assigned to a typed policy or operational failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum FailureClass {
    /// Re-execution can succeed without configuration or code changes.
    Transient,
    /// Re-execution requires an external state change.
    Permanent,
    /// The failure contradicts an internal correctness invariant.
    InvariantViolation,
}

/// Classified failure accepted by the task runtime boundary.
///
/// The class decides what the runtime does next; the cause is the typed error
/// the operation failed with, rendered only when the runtime records the
/// failure in health and lifecycle reporting.
#[derive(Debug, Error)]
#[error("{cause}")]
pub struct TaskFailure<E> {
    /// Runtime classification supplied by composition.
    class: FailureClass,
    /// The typed error the operation failed with.
    #[source]
    cause: E,
}

impl<E> TaskFailure<E> {
    /// Construct a failure of the given class.
    pub const fn new(class: FailureClass, cause: E) -> TaskFailure<E> {
        TaskFailure { class, cause }
    }

    /// Construct a failure a later attempt may recover from.
    pub const fn transient(cause: E) -> TaskFailure<E> {
        TaskFailure::new(FailureClass::Transient, cause)
    }

    /// Construct a failure that needs an external state change before a retry can succeed.
    pub const fn permanent(cause: E) -> TaskFailure<E> {
        TaskFailure::new(FailureClass::Permanent, cause)
    }

    /// Construct a failure that contradicts an internal correctness invariant.
    pub const fn invariant_violation(cause: E) -> TaskFailure<E> {
        TaskFailure::new(FailureClass::InvariantViolation, cause)
    }

    /// Return the runtime failure class.
    #[must_use]
    pub const fn class(&self) -> FailureClass {
        self.class
    }

    /// Return the typed cause.
    #[must_use]
    pub const fn cause(&self) -> &E {
        &self.cause
    }

    /// Keep the class and wrap the cause into the error type of a wider operation.
    #[must_use]
    pub fn map_cause<F>(self, wrap: impl FnOnce(E) -> F) -> TaskFailure<F> {
        TaskFailure::new(self.class, wrap(self.cause))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        failure::{FailureClass, TaskFailure},
        runtime_failure::RuntimeFailure,
    };
    use std::error::Error;

    #[derive(Debug, thiserror::Error)]
    #[error("wider operation failed")]
    struct Wider(#[source] RuntimeFailure);

    #[test]
    fn a_failure_renders_and_chains_its_typed_cause() {
        let failure = TaskFailure::permanent(RuntimeFailure::RunNowChannelClosed);
        assert_eq!(failure.class(), FailureClass::Permanent);
        assert_eq!(failure.to_string(), "scheduled run-now channel closed while task was active");
        assert!(failure.source().is_some(), "the typed cause is the source");
    }

    #[test]
    fn mapping_the_cause_keeps_the_class() {
        let failure = TaskFailure::transient(RuntimeFailure::ResidentCompleted).map_cause(Wider);
        assert_eq!(failure.class(), FailureClass::Transient);
        assert!(matches!(failure.cause(), Wider(RuntimeFailure::ResidentCompleted)));
    }
}
