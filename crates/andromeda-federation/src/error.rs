//! Errors produced by context-source federation.

use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_registration::error::RegistrationError;
use std::time::Duration;
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
    #[error("federation conflict: {detail}")]
    Conflict {
        /// Human-readable conflict detail.
        detail: String,
    },
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
        /// or the transport failure.
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
    #[error("federation invariant failed: {detail}")]
    Internal {
        /// Internal diagnostic detail.
        detail: String,
    },
}

impl FederationError {
    /// Construct an exclusive-registration conflict.
    pub fn conflict(detail: impl Into<String>) -> FederationError {
        FederationError::Conflict { detail: detail.into() }
    }

    /// Construct an internal invariant error.
    pub fn internal(detail: impl Into<String>) -> FederationError {
        FederationError::Internal { detail: detail.into() }
    }
}
