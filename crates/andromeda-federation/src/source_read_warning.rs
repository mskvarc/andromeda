//! The clause 6.3.17 warnings of a forwarded read that failed or came back partial.

use crate::outcome::TransportFailure;
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
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
    /// This broker could not build the request, so the source was never
    /// contacted. The cause is the broker's own and is logged, not reported.
    #[error("the request for context source {registration} could not be prepared and was not sent")]
    Unprepared {
        /// The registration the request was meant for.
        registration: &'a Identifier,
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

impl SourceReadWarning<'_> {
    /// The warning this is reported as.
    ///
    /// Table 6.3.17-1 sorts by what came back: an error response is 299, a
    /// response whose payload cannot be used is 111, and no response at all,
    /// or a loop, is 199.
    pub(crate) fn into_warning(self) -> NgsiLdWarning {
        let code = match self {
            SourceReadWarning::ErrorStatus { .. } => WarnCode::MiscellaneousPersistentWarning,
            SourceReadWarning::UnreadableContentRange { .. } => WarnCode::RevalidationFailed,
            SourceReadWarning::Transport { .. } | SourceReadWarning::Unprepared { .. } | SourceReadWarning::LoopDetected { .. } => {
                WarnCode::MiscellaneousWarning
            }
        };
        NgsiLdWarning::new(code, self)
    }
}
