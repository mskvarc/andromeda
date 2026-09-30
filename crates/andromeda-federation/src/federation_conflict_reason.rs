//! Why a single forwarded write is reported as a conflict.
//!
//! [`FederationError::Conflict`](crate::error::FederationError::Conflict) carries
//! one of these. A conflict this broker reasons about names its registration
//! with a typed identifier; a conflict the remote source declared carries that
//! source's own answer. The clauses (5.6.1.4, 6.3.17)
//! stay in doc comments.

use crate::remote_answer::RemoteAnswer;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// Why a single exclusive or redirect source could not accept the write.
#[derive(Debug, Error)]
pub enum FederationConflictReason {
    /// The single registered source covers the data but does not advertise the
    /// operation the request needs of it (clauses 5.6.1.4 and following).
    #[error("registration {registration} covers the data but does not advertise the requested operation")]
    NotAdvertised {
        /// The registration that covers the data without advertising the operation.
        registration: Identifier,
    },
    /// The single registered source itself refused the write with an HTTP 409.
    ///
    /// The answer carries the source's own problem document; clause 6.3.17
    /// asks for the most informative error.
    #[error(transparent)]
    RemoteRejected(Box<RemoteAnswer>),
}
