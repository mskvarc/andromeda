//! A refusal this broker reasons about names its registration; a refusal the remote source declared carries that source's own answer.

use crate::remote_answer::RemoteAnswer;
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
    /// The answer carries the source's own problem document; clause 6.3.17
    /// asks for the most informative error.
    #[error(transparent)]
    RemoteUnsupported(Box<RemoteAnswer>),
}
