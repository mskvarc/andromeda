//! Why a context source did not complete a forwarded subscription operation
//! (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).

use crate::{outcome::TransportFailure, remote_answer::RemoteAnswer};
use thiserror::Error;

/// Why one subscription forward did not complete at the source.
#[derive(Debug, Clone, Error)]
pub enum SubscriptionForwardFailure {
    /// The source answered with an error status, and with the problem detail
    /// that names the member it refused when it sent one.
    #[error(transparent)]
    Answered(RemoteAnswer),
    /// No answer was obtained from the source.
    #[error(transparent)]
    Transport(TransportFailure),
    /// The registration loops back into this broker, so the source was not
    /// contacted (clauses 4.3.6.4 and 6.3.17).
    #[error("the registration loops back into this broker")]
    LoopDetected,
    /// The registration does not advertise the operation, so the source was
    /// not contacted.
    #[error("the registration does not advertise the operation")]
    NotAdvertised,
}
