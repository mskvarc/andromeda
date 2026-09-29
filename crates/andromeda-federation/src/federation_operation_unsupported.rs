//! Why a single registered source cannot perform the forwarded operation.
//!
//! [`FederationError::OperationNotSupported`](crate::error::FederationError::OperationNotSupported)
//! carries one of these (422; clause 5.6.8.4). A refusal this broker reasons
//! about names its registration; a refusal the remote source declared carries
//! that source's own problem detail, relayed verbatim.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// Why a single exclusive or redirect source cannot serve the operation.
#[derive(Debug, Error)]
pub enum FederationOperationUnsupported {
    /// An upsert follow-up found the entity present at the single source, which
    /// advertises no operation to update it.
    #[error("registration {registration} reported the entity exists and advertises no operation to update it")]
    UpsertFollowUpNotAdvertised {
        /// The registration that holds the entity without an update operation.
        registration: Identifier,
    },
    /// The single registered source itself refused the operation with an HTTP 422.
    ///
    /// The detail is the source's own problem detail, relayed verbatim; it is
    /// not broker-authored (clause 6.3.17 asks for the most informative error).
    #[error("{detail}")]
    RemoteUnsupported {
        /// The remote source's relayed HTTP 422 detail. Opaque upstream text.
        detail: String,
    },
}
