//! Errors produced by context-source registration policy and persistence.

use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::error::QueryError;
use thiserror::Error;

/// A failure while validating, applying, or persisting a registration operation.
#[derive(Debug, Error)]
pub enum RegistrationError {
    /// The request does not satisfy the registration contract.
    #[error("invalid context-source registration: {detail}")]
    Validation {
        /// Human-readable validation detail.
        detail: String,
    },
    /// The requested registration does not exist.
    #[error("context-source registration not found: {identifier}")]
    NotFound {
        /// Missing registration identifier.
        identifier: Uri,
    },
    /// A registration already uses the requested identifier.
    #[error("context-source registration already exists: {identifier}")]
    AlreadyExists {
        /// Conflicting registration identifier.
        identifier: Uri,
    },
    /// A proxied registration overlaps an existing registration or a local
    /// entity (clauses 5.9.2.4 and 5.9.3.4, `Conflict`).
    #[error("context-source registration conflicts with existing context data: {detail}")]
    Conflict {
        /// Which registration or entity the footprint collides with.
        detail: String,
    },
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// Query compilation failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// JSON representation processing failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// Registration identifier generation failed.
    #[error(transparent)]
    Identifier(#[from] urn_rs::Error),
    /// The persistence adapter could not complete the requested operation.
    #[error("registration persistence failed: {detail}")]
    Persistence {
        /// Adapter-specific diagnostic detail.
        detail: String,
    },
    /// An internal registration invariant could not be satisfied.
    #[error("registration invariant failed: {detail}")]
    Internal {
        /// Internal diagnostic detail.
        detail: String,
    },
}

impl RegistrationError {
    /// Construct a request-validation error.
    pub fn csr_validation(detail: impl Into<String>) -> RegistrationError {
        RegistrationError::Validation { detail: detail.into() }
    }

    /// Construct a missing-registration error.
    #[must_use]
    pub fn not_found(identifier: &Uri) -> RegistrationError {
        RegistrationError::NotFound {
            identifier: identifier.clone(), // clone: the error must own the borrowed request identifier.
        }
    }

    /// Construct an identifier-conflict error.
    #[must_use]
    pub fn already_exists(identifier: &Uri) -> RegistrationError {
        RegistrationError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error must outlive the borrowed request identifier.
        }
    }

    /// Construct a proxied-registration overlap error.
    pub fn conflict(detail: impl Into<String>) -> RegistrationError {
        RegistrationError::Conflict { detail: detail.into() }
    }

    /// Construct a persistence-adapter error.
    pub fn persistence(detail: impl Into<String>) -> RegistrationError {
        RegistrationError::Persistence { detail: detail.into() }
    }

    /// Construct an internal invariant error.
    pub fn internal(detail: impl Into<String>) -> RegistrationError {
        RegistrationError::Internal { detail: detail.into() }
    }
}
