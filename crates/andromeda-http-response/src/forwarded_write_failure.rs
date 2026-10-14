//! The detail text of a forwarded write that failed without a problem document of its own (clause 6.3.17).

use andromeda_federation::{outcome::TransportFailure, remote_answer::RemoteAnswer};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// Why a forwarded write failed, as the clause 5.2.17 `detail` of the error it contributes.
#[derive(Debug, Error)]
pub(crate) enum ForwardedWriteFailure<'a> {
    /// The source answered an error status that carried no problem document.
    #[error("{0}")]
    RemoteStatus(RemoteAnswer),
    /// No HTTP response arrived.
    #[error("{0}")]
    Transport(&'a TransportFailure),
    /// The registration loops back into the request's `Via` chain.
    #[error("registration {registration} loops back into the request's Via chain")]
    LoopDetected {
        /// The registration that loops.
        registration: &'a Identifier,
    },
    /// The registration covers the data but does not advertise the operation.
    #[error("registration {registration} covers the data but does not advertise the requested operation")]
    NotAdvertised {
        /// The registration that lacks the operation.
        registration: &'a Identifier,
    },
    /// An upsert fell back to a create the source refused as existing, and no operation updates the entity.
    #[error("registration {registration} reported the entity exists and advertises no operation to update it")]
    UpsertFollowUpNotAdvertised {
        /// The registration that lacks the update operation.
        registration: &'a Identifier,
    },
}
