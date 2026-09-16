//! Errors produced by subscription policy and persistence.

use crate::{subscription_diagnostic::SubscriptionDiagnostic, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::error::QueryError;
use std::fmt::Display;
use thiserror::Error;

/// A failure while validating, applying, compiling, or persisting a subscription.
#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// The request does not satisfy the subscription contract.
    #[error("invalid subscription: {0}")]
    Validation(#[from] SubscriptionValidationReason),
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
    #[error("subscription persistence failed: {0}")]
    Persistence(SubscriptionDiagnostic),
    /// An internal subscription invariant could not be satisfied.
    #[error("subscription invariant failed: {0}")]
    Internal(SubscriptionDiagnostic),
}

impl SubscriptionError {
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

    /// Construct a persistence-adapter failure from its masked diagnostic source.
    ///
    /// The detail is captured for operators and masked before any client sees
    /// it, so this takes `impl Display` — a typed adapter error is handed over
    /// directly rather than pre-rendered to a string at the call site.
    pub fn persistence(source: impl Display) -> SubscriptionError {
        SubscriptionError::Persistence(SubscriptionDiagnostic::new(source))
    }

    /// Construct an internal-invariant failure from its masked diagnostic source.
    pub fn internal(source: impl Display) -> SubscriptionError {
        SubscriptionError::Internal(SubscriptionDiagnostic::new(source))
    }
}
