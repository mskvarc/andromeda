//! The clause 6.3.17 warnings of a forwarded read that failed or came back partial.

use crate::outcome::TransportFailure;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// What a forwarded read of one registration reports as a warning.
#[derive(Debug, Error)]
pub(crate) enum SourceReadWarning<'a> {
    /// The source answered an error status other than 404.
    #[error("context source {registration} returned HTTP {status}")]
    ErrorStatus {
        /// The registration that answered.
        registration: &'a Identifier,
        /// The status it answered with.
        status: u16,
    },
    /// No HTTP response arrived.
    #[error("context source {registration} did not respond: {failure}")]
    Transport {
        /// The registration that was contacted.
        registration: &'a Identifier,
        /// Why no response arrived.
        failure: &'a TransportFailure,
    },
    /// The registration's alias is already in the `Via` chain.
    #[error("registration loop detected for {registration}")]
    LoopDetected {
        /// The registration that loops.
        registration: &'a Identifier,
    },
    /// A 206 answer carries no readable `Content-Range`.
    #[error("context source {registration} answered 206 without a readable Content-Range; its part of the history may be incomplete")]
    UnreadableContentRange {
        /// The registration that answered.
        registration: &'a Identifier,
    },
}
