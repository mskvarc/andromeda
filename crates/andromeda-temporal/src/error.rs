//! Failures produced by temporal policy and its storage ports.

use crate::{
    temporal_invariant::TemporalInvariant,
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_operation_unsupported::TemporalOperationUnsupported,
    temporal_validation_reason::TemporalValidationReason,
};
use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
use andromeda_entity::error::EntityError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_render::temporal::values::TemporalValuesError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
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
    Persistence(#[source] PersistenceFailure),
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
    Rendering(#[source] TemporalValuesError),
    /// An internal temporal invariant could not be satisfied.
    #[error("temporal invariant failed: {0}")]
    Internal(#[from] TemporalInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// The commit port reports a refused submission separately from its own faults,
/// and collapsing both into one variant would answer an unusable request with
/// `InternalError` and invite the client to retry it forever (clause 5.5.6).
impl From<CommitError> for TemporalError {
    fn from(error: CommitError) -> TemporalError {
        match error {
            CommitError::Submission(refusal) => TemporalError::Validation(TemporalValidationReason::Refused(refusal)),
            // Saturation is backpressure the client can retry, so it keeps its
            // own category rather than being folded into the internal-fault
            // `Commit` variant.
            CommitError::StoreSaturated => TemporalError::StoreSaturated,
            CommitError::TransactionContended => TemporalError::TransactionContended,
            failure @ (CommitError::DuplicateEntity { .. }
            | CommitError::Persistence(_)
            | CommitError::Invariant(_)
            | CommitError::ReplayLeaseLost { .. }
            | CommitError::UnexpectedOutcomeCount { .. }) => TemporalError::Commit(failure),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store and a transaction rolled back under contention are
/// backpressure the client can retry, so each keeps its own retryable category;
/// only a fault becomes [`TemporalError::Persistence`].
impl From<PersistenceFailure> for TemporalError {
    fn from(failure: PersistenceFailure) -> TemporalError {
        match failure {
            PersistenceFailure::Saturated => TemporalError::StoreSaturated,
            PersistenceFailure::RolledBack => TemporalError::TransactionContended,
            fault @ (PersistenceFailure::Unavailable
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
            | PersistenceFailure::CapacityExceeded
            | PersistenceFailure::SchemaMismatch
            | PersistenceFailure::ContractBroken) => TemporalError::Persistence(fault),
        }
    }
}

/// Manual rather than `#[from]`: a refused submission is a validation failure,
/// two layers down from the error it becomes.
impl From<SubmissionRefusal> for TemporalError {
    fn from(refusal: SubmissionRefusal) -> TemporalError {
        TemporalError::Validation(TemporalValidationReason::Refused(refusal))
    }
}

impl TemporalError {
    /// Construct an over-the-bound temporal-extension failure.
    #[must_use]
    pub const fn too_many_results(matched: u64, maximum: u64) -> TemporalError {
        TemporalError::TooManyResults { matched, maximum }
    }

    /// Classify a failed entity operation a temporal operation depends on.
    ///
    /// Backpressure and a store fault keep their own categories; any other
    /// entity failure is recorded for operators and reported as the broken
    /// invariant it is, since the broker itself drove the entity operation.
    #[must_use]
    pub fn from_entity_failure(error: EntityError) -> TemporalError {
        match error {
            EntityError::StoreSaturated => TemporalError::StoreSaturated,
            EntityError::TransactionContended => TemporalError::TransactionContended,
            EntityError::Persistence(failure) => TemporalError::Persistence(failure),
            other @ (EntityError::Validation(_)
            | EntityError::NotFound { .. }
            | EntityError::TypeNotFound { .. }
            | EntityError::AttributeNotFound { .. }
            | EntityError::MemberNotFound(_)
            | EntityError::AlreadyExists { .. }
            | EntityError::Contention { .. }
            | EntityError::OperationNotSupported(_)
            | EntityError::JsonLd(_)
            | EntityError::Query(_)
            | EntityError::Request(_)
            | EntityError::Representation(_)
            | EntityError::Commit(_)
            | EntityError::Internal(_)) => {
                tracing::error!(error = %other, "an entity operation a temporal operation depends on failed");
                TemporalInvariant::EntityOperation.into()
            }
        }
    }

    /// Whether this failure deliberately reports a missing temporal resource.
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(self, TemporalError::NotFound(_))
    }
}
