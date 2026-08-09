//! Errors produced by context-source federation.

use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_registration::error::RegistrationError;
use thiserror::Error;

/// A failure while selecting, contacting, or combining context sources.
#[derive(Debug, Error)]
pub enum FederationError {
    /// Federation cannot satisfy mutually exclusive registration rules.
    #[error("federation conflict: {detail}")]
    Conflict {
        /// Human-readable conflict detail.
        detail: String,
    },
    /// A selected context source failed to fulfil a mandatory operation.
    #[error("context-source gateway failure: {detail}")]
    Gateway {
        /// Human-readable upstream failure detail.
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
    /// An upstream HTTP request failed.
    #[error(transparent)]
    Request(#[from] reqwest::Error),
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

    /// Construct a mandatory upstream failure.
    pub fn context_source_gateway(detail: impl Into<String>) -> FederationError {
        FederationError::Gateway { detail: detail.into() }
    }

    /// Construct an internal invariant error.
    pub fn internal(detail: impl Into<String>) -> FederationError {
        FederationError::Internal { detail: detail.into() }
    }
}
