//! Errors produced by subscription policy and persistence.

use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::error::QueryError;
use thiserror::Error;

/// A failure while validating, applying, compiling, or persisting a subscription.
#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// The request does not satisfy the subscription contract.
    #[error("invalid subscription: {detail}")]
    Validation {
        /// Human-readable validation detail.
        detail: String,
    },
    /// The requested subscription does not exist.
    #[error("subscription not found: {identifier}")]
    NotFound {
        /// Missing subscription identifier.
        identifier: Uri,
    },
    /// A subscription already uses the requested identifier.
    #[error("subscription already exists: {identifier}")]
    AlreadyExists {
        /// Conflicting subscription identifier.
        identifier: Uri,
    },
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// Query parsing or expansion failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// JSON representation processing failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// Subscription identifier generation failed.
    #[error(transparent)]
    Identifier(#[from] urn_rs::Error),
    /// The persistence adapter could not complete the operation.
    #[error("subscription persistence failed: {detail}")]
    Persistence {
        /// Adapter-specific diagnostic detail.
        detail: String,
    },
    /// An internal subscription invariant could not be satisfied.
    #[error("subscription invariant failed: {detail}")]
    Internal {
        /// Internal diagnostic detail.
        detail: String,
    },
}

impl SubscriptionError {
    /// Construct a request-validation error.
    pub fn validation(detail: impl Into<String>) -> SubscriptionError {
        SubscriptionError::Validation { detail: detail.into() }
    }

    /// Construct a missing-subscription error.
    #[must_use]
    pub fn not_found(identifier: &Uri) -> SubscriptionError {
        SubscriptionError::NotFound {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct an identifier-conflict error.
    #[must_use]
    pub fn already_exists(identifier: &Uri) -> SubscriptionError {
        SubscriptionError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct a persistence-adapter error.
    pub fn persistence(detail: impl Into<String>) -> SubscriptionError {
        SubscriptionError::Persistence { detail: detail.into() }
    }

    /// Construct an internal-invariant error.
    pub fn internal(detail: impl Into<String>) -> SubscriptionError {
        SubscriptionError::Internal { detail: detail.into() }
    }
}
