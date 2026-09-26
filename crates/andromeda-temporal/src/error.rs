//! Failures produced by temporal policy and its storage ports.

use crate::{
    temporal_diagnostic::TemporalDiagnostic,
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_operation_unsupported::TemporalOperationUnsupported,
    temporal_validation_reason::TemporalValidationReason,
};
use andromeda_context_commit::error::CommitError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_request::error::ParameterError;
use std::fmt::Display;
use thiserror::Error;

/// A failure while compiling, reading, rendering, or mutating temporal history.
#[derive(Debug, Error)]
pub enum TemporalError {
    /// The temporal request or representation is semantically invalid.
    #[error("invalid temporal operation: {0}")]
    Validation(#[from] TemporalValidationReason),
    /// The requested temporal resource does not exist.
    #[error("temporal resource not found: {0}")]
    NotFound(#[from] TemporalNotFoundReason),
    /// The selected history capability cannot safely answer the request.
    #[error("temporal operation is not supported: {0}")]
    OperationNotSupported(#[from] TemporalOperationUnsupported),
    /// A bounded temporal extension would produce more results than its
    /// configured maximum admits.
    ///
    /// Reported off a pre-count, so an oversized operation is refused whole
    /// rather than partially executed. Two extensions raise it: the
    /// `temporal_delete_by_query` bound on the history instances a purge would
    /// remove, and the `temporal_aggregation_grouping` bound on the distinct
    /// groups a grouped aggregation would emit. Classified as `TooManyResults`
    /// (403), mirroring the clause 5.6.21.4 bound the standard Purge raises.
    #[error("temporal operation would produce {matched} results, over the configured maximum of {maximum}")]
    TooManyResults {
        /// The number of results the operation matched (instances or groups).
        matched: u64,
        /// The configured upper bound.
        maximum: u64,
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
    /// An atomic context write failed for a reason the client cannot act on.
    ///
    /// A commit that refused the submitted content never reaches this variant:
    /// [`From<CommitError>`] turns that refusal into [`TemporalError::Validation`],
    /// which is what clause 5.5.6 reports as `BadRequestData`.
    #[error(transparent)]
    Commit(CommitError),
    /// A persistence adapter could not complete the operation.
    #[error("temporal persistence failed: {0}")]
    Persistence(TemporalDiagnostic),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// The datastore rolled the transaction back to break a deadlock or a
    /// serialization conflict with a concurrent transaction.
    ///
    /// Not a fault: the whole transaction was undone, so the well-formed
    /// request can be retried unchanged. It is answered 503 with
    /// `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the operation was rolled back by a concurrent transaction; retry shortly")]
    TransactionContended,
    /// Temporal representation construction failed.
    #[error("temporal rendering failed: {0}")]
    Rendering(TemporalDiagnostic),
    /// An internal temporal invariant could not be satisfied.
    #[error("temporal invariant failed: {0}")]
    Internal(TemporalDiagnostic),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// The commit port reports a refused submission separately from its own faults,
/// and collapsing both into one variant would answer an unusable request with
/// `InternalError` and invite the client to retry it forever (clause 5.5.6).
impl From<CommitError> for TemporalError {
    fn from(error: CommitError) -> TemporalError {
        match error {
            // The commit port cannot depend on this crate's error types without a
            // cycle, so a refused submission arrives as opaque prose relayed into
            // the typed validation reason rather than as a typed cause.
            CommitError::Submission { detail } => TemporalError::Validation(TemporalValidationReason::CommitRefused { detail }),
            // Saturation is backpressure the client can retry, so it keeps its
            // own category rather than being folded into the internal-fault
            // `Commit` variant.
            CommitError::StoreSaturated => TemporalError::StoreSaturated,
            CommitError::TransactionContended => TemporalError::TransactionContended,
            failure @ (CommitError::DuplicateEntity { .. }
            | CommitError::Persistence { .. }
            | CommitError::Invariant { .. }
            | CommitError::UnexpectedOutcomeCount { .. }) => TemporalError::Commit(failure),
        }
    }
}

impl TemporalError {
    /// Construct an over-the-bound temporal-extension failure.
    #[must_use]
    pub const fn too_many_results(matched: u64, maximum: u64) -> TemporalError {
        TemporalError::TooManyResults { matched, maximum }
    }

    /// Construct a persistence failure from its masked diagnostic source.
    ///
    /// The detail is captured for operators and masked before any client sees
    /// it, so this takes `impl Display` — a typed adapter error is handed over
    /// directly rather than pre-rendered to a string at the call site.
    pub fn persistence(source: impl Display) -> TemporalError {
        TemporalError::Persistence(TemporalDiagnostic::new(source))
    }

    /// Construct an internal rendering failure from its masked diagnostic source.
    pub fn rendering(source: impl Display) -> TemporalError {
        TemporalError::Rendering(TemporalDiagnostic::new(source))
    }

    /// Construct an internal invariant failure from its masked diagnostic source.
    pub fn internal(source: impl Display) -> TemporalError {
        TemporalError::Internal(TemporalDiagnostic::new(source))
    }

    /// Whether this failure deliberately reports a missing temporal resource.
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(self, TemporalError::NotFound(_))
    }
}
