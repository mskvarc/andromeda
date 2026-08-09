//! Failures produced by temporal policy and its storage ports.

use andromeda_context_commit::error::CommitError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_request::error::ParameterError;
use thiserror::Error;

/// A failure while compiling, reading, rendering, or mutating temporal history.
#[derive(Debug, Error)]
pub enum TemporalError {
    /// The temporal request or representation is semantically invalid.
    #[error("invalid temporal operation: {detail}")]
    Validation {
        /// Deliberate validation detail.
        detail: String,
    },
    /// The requested temporal resource does not exist.
    #[error("temporal resource not found: {detail}")]
    NotFound {
        /// Deliberate missing-resource detail.
        detail: String,
    },
    /// The selected history capability cannot safely answer the request.
    #[error("temporal operation is not supported: {detail}")]
    OperationNotSupported {
        /// Unsupported capability detail.
        detail: String,
    },
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// NGSI-LD query compilation failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// Query-parameter interpretation failed.
    #[error(transparent)]
    Request(#[from] ParameterError),
    /// An atomic context write failed.
    #[error(transparent)]
    Commit(#[from] CommitError),
    /// A persistence adapter could not complete the operation.
    #[error("temporal persistence failed: {detail}")]
    Persistence {
        /// Adapter diagnostic retained for operators.
        detail: String,
    },
    /// Temporal representation construction failed.
    #[error("temporal rendering failed: {detail}")]
    Rendering {
        /// Internal rendering diagnostic.
        detail: String,
    },
    /// An internal temporal invariant could not be satisfied.
    #[error("temporal invariant failed: {detail}")]
    Internal {
        /// Internal diagnostic detail.
        detail: String,
    },
}

impl TemporalError {
    /// Construct a validation failure.
    pub fn validation(detail: impl Into<String>) -> TemporalError {
        TemporalError::Validation { detail: detail.into() }
    }

    /// Construct a missing-resource failure.
    pub fn not_found(detail: impl Into<String>) -> TemporalError {
        TemporalError::NotFound { detail: detail.into() }
    }

    /// Construct an unsupported-capability failure.
    pub fn operation_not_supported(detail: impl Into<String>) -> TemporalError {
        TemporalError::OperationNotSupported { detail: detail.into() }
    }

    /// Construct a persistence failure at an adapter boundary.
    pub fn persistence(detail: impl Into<String>) -> TemporalError {
        TemporalError::Persistence { detail: detail.into() }
    }

    /// Construct an internal rendering failure.
    pub fn rendering(detail: impl Into<String>) -> TemporalError {
        TemporalError::Rendering { detail: detail.into() }
    }

    /// Construct an internal invariant failure.
    pub fn internal(detail: impl Into<String>) -> TemporalError {
        TemporalError::Internal { detail: detail.into() }
    }

    /// Whether this failure deliberately reports a missing temporal resource.
    pub const fn is_not_found(&self) -> bool {
        matches!(self, TemporalError::NotFound { .. })
    }
}
