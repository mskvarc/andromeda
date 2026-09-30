//! Why a single forwarded request failed as a bad gateway (clause 6.3.17).

use crate::{outcome::TransportFailure, remote_answer::RemoteAnswer};
use thiserror::Error;

/// Why the single registered source could not complete a forwarded operation.
#[derive(Debug, Clone, Error)]
pub enum GatewayFailure {
    /// The source answered with a status the operation cannot succeed under.
    #[error(transparent)]
    Answered(RemoteAnswer),
    /// No answer was obtained from the source.
    #[error(transparent)]
    Transport(TransportFailure),
}
