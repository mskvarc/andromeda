//! Errors produced by context-source federation.

use crate::{
    federation_conflict_reason::FederationConflictReason,
    federation_diagnostic::FederationDiagnostic,
    federation_operation_unsupported::FederationOperationUnsupported,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_registration::error::RegistrationError;
use std::{fmt::Display, time::Duration};
use thiserror::Error;

/// A failure while selecting, contacting, or combining context sources.
///
/// The four single-source variants are the clause 6.3.17 errors for an unsafe
/// method whose data lives in one exclusive or redirect registration: the
/// forwarded failure is the operation's failure, so it is reported with the
/// status and detail the clause assigns rather than folded into a 207.
#[derive(Debug, Error)]
pub enum FederationError {
    /// Federation cannot satisfy mutually exclusive registration rules.
    #[error("federation conflict: {0}")]
    Conflict(FederationConflictReason),
    /// The single registered source cannot perform the operation the request
    /// needs of it (422; clause 5.6.8.4).
    #[error("operation not supported at the registered source: {0}")]
    OperationNotSupported(FederationOperationUnsupported),
    /// The single registered source redirects back onto this broker (508).
    #[error("registration {registration} loops back onto this context broker")]
    LoopDetected {
        /// The looping registration.
        registration: Uri,
    },
    /// The single registered source failed to respond in time (504).
    #[error("registration {registration} did not respond within {}ms", timeout.as_millis())]
    GatewayTimeout {
        /// The silent registration.
        registration: Uri,
        /// The timeout that elapsed.
        timeout: Duration,
    },
    /// The resource was not found within the single registered source (404).
    #[error("registration {registration} holds no such resource")]
    RemoteNotFound {
        /// The registration that answered 404.
        registration: Uri,
    },
    /// The single forwarded request failed for any other reason (502).
    #[error("registration {registration} could not complete the operation: {detail}")]
    BadGateway {
        /// The failing registration.
        registration: Uri,
        /// The status the source answered with, when it answered at all.
        status: Option<u16>,
        /// The most informative detail available: the source's problem detail,
        /// or the transport failure. Relayed verbatim from the source and not
        /// broker-authored, so it stays an opaque `String` (clause 6.3.17).
        detail: String,
    },
    /// Registration discovery failed.
    #[error(transparent)]
    Registration(#[from] RegistrationError),
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// Query processing failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// An upstream representation was invalid.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// An internal federation invariant could not be satisfied.
    #[error("federation invariant failed: {0}")]
    Internal(FederationDiagnostic),
}

impl FederationError {
    /// Construct an internal invariant error from its masked diagnostic source.
    ///
    /// The detail is captured for operators and masked before any client sees
    /// it, so this takes `impl Display` — a typed source error is handed over
    /// directly rather than pre-rendered to a string at the call site.
    pub fn internal(source: impl Display) -> FederationError {
        FederationError::Internal(FederationDiagnostic::new(source))
    }
}
