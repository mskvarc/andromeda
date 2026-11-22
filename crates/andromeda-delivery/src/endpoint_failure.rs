//! Why one notification did not reach the endpoint it was sent to.

use crate::{error::TransportError, failure_class::FailureClass, outcome::EndpointRejection};

/// A failed delivery attempt: what the endpoint answered, or why it was not
/// reached.
///
/// Its text is the one the dispatcher logs, so a caller that stores it records
/// what the log line says.
#[derive(Debug, thiserror::Error)]
pub enum EndpointFailure {
    /// The endpoint was reached and refused the notification.
    #[error("the endpoint answered {reason}")]
    Rejected {
        /// What the endpoint answered.
        reason: EndpointRejection,
        /// What the answer says about attempting the same delivery again.
        class: FailureClass,
    },
    /// The endpoint was not reached, or the notification was refused before it
    /// was sent.
    #[error(transparent)]
    Transport(#[from] TransportError),
}

impl EndpointFailure {
    /// What the failure says about attempting the same delivery again.
    #[must_use]
    pub const fn class(&self) -> FailureClass {
        match self {
            EndpointFailure::Rejected { class, .. } => *class,
            EndpointFailure::Transport(error) => error.class(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        endpoint_failure::EndpointFailure,
        error::TransportError,
        failure_class::FailureClass,
        outcome::EndpointRejection,
        oversized_notification::OversizedNotification,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_rejection_names_the_status_the_endpoint_answered_and_keeps_its_class() {
        let rejected = EndpointFailure::Rejected {
            reason: EndpointRejection::HttpStatus { status: 503 },
            class: FailureClass::Unavailable,
        };
        assert_eq!(rejected.to_string(), "the endpoint answered HTTP 503");
        assert_eq!(rejected.class(), FailureClass::Unavailable);
    }

    #[test]
    fn a_transport_failure_reads_as_the_transport_error_and_keeps_its_class() {
        let refusal = OversizedNotification::MaxHttpBody { size: 2048, limit: 1024 };
        let failure = EndpointFailure::from(TransportError::from(refusal));
        assert_eq!(failure.to_string(), refusal.to_string());
        assert_eq!(failure.class(), FailureClass::Permanent);
    }
}
