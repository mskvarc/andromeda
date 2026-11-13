//! Failures produced by temporal policy and its storage ports.

use crate::{
    result_bound::ResultBound,
    temporal_invariant::TemporalInvariant,
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_operation_unsupported::TemporalOperationUnsupported,
    temporal_validation_reason::TemporalValidationReason,
    unsettled_history::UnsettledHistory,
};
use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
use andromeda_entity::error::EntityError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_compaction::compaction_error::CompactionError;
use andromeda_ngsi_ld_entity::{history::current_state_fold::CurrentStateFoldError, operation::mutation_error::MutationError, refusal::EntityRefusal};
use andromeda_ngsi_ld_expansion::error::ExpansionError;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_representation::{attribute_nesting::AttributeNestingExceeded, payload_budget::PayloadBudgetExceeded};
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_persistence_failure::{persistence_failure::PersistenceFailure, retryability::RetryCause};
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
    /// A temporal operation counts more than a configured bound admits.
    ///
    /// The operation is refused whole rather than partially executed: the
    /// `temporal_delete_by_query` bound on the history instances a purge would
    /// remove, counted before any is removed; the
    /// `temporal_aggregation_grouping` bound on the distinct groups a grouped
    /// aggregation would emit; and `temporal.max_edge_group` on the instances
    /// of one attribute sharing the instant at a page edge, which a page
    /// carries whole. Classified as `TooManyResults` (403), the clause 5.5.6
    /// refusal whose threshold is the implementation's.
    #[error("temporal operation counts {matched} {bound}, over the configured maximum of {maximum}")]
    TooManyResults {
        /// The number the operation counted.
        matched: u64,
        /// The configured upper bound.
        maximum: u64,
        /// What was counted, and the setting bounding it.
        bound: ResultBound,
    },
    /// The written payload exceeds the budget an entity is admitted under
    /// before it is expanded (clause 6.3.2, "Request Entity too large").
    #[error(transparent)]
    PayloadBudget(#[from] PayloadBudgetExceeded),
    /// A written attribute nests deeper than the broker reads back what it
    /// stores; the HTTP binding answers it with `BadRequestData`.
    #[error(transparent)]
    AttributeNesting(#[from] AttributeNestingExceeded),
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
    #[error("{}", RetryCause::Saturated)]
    StoreSaturated,
    /// The datastore could not be reached, or the connection to it was lost.
    ///
    /// Not a fault of the request: nothing of it is known to have persisted, so
    /// the identical request can be sent again once the store is back. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("{}", RetryCause::Unreachable)]
    StoreUnavailable,
    /// The datastore rolled the transaction back to break a deadlock or a
    /// serialization conflict with a concurrent transaction.
    ///
    /// Not a fault: the whole transaction was undone, so the well-formed
    /// request can be retried unchanged. It is answered 503 with
    /// `Retry-After`, never the 500 a broker malfunction earns.
    #[error("{}", RetryCause::Contended)]
    TransactionContended,
    /// The mutation barrier's bound ran out before the entity's queued history
    /// was applied: another delivery owner still held some of it, or more was
    /// queued than the barrier could apply within the bound.
    ///
    /// Not a fault: the history is being applied, what the barrier applied
    /// stays applied, and the mutation can run once the rest is or once the
    /// holder's lease lapses. It is answered 503 with a `Retry-After` of the
    /// holder's remaining lease, at least one second, never the 500 a broker
    /// malfunction earns.
    #[error("the entity's queued temporal history is still being applied; retry shortly")]
    HistoryUnsettled(UnsettledHistory),
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
            CommitError::StoreUnavailable => TemporalError::StoreUnavailable,
            failure @ (CommitError::DuplicateEntity { .. }
            | CommitError::Persistence(_)
            | CommitError::Invariant(_)
            | CommitError::ReplayLeaseLost { .. }
            | CommitError::UnexpectedOutcomeCount { .. }) => TemporalError::Commit(failure),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store, a transaction rolled back under contention and a store
/// that cannot be reached are conditions the client can retry, so each keeps
/// its own retryable category; only a fault becomes
/// [`TemporalError::Persistence`].
impl From<PersistenceFailure> for TemporalError {
    fn from(failure: PersistenceFailure) -> TemporalError {
        match failure {
            PersistenceFailure::Saturated => TemporalError::StoreSaturated,
            PersistenceFailure::Unavailable => TemporalError::StoreUnavailable,
            PersistenceFailure::RolledBack => TemporalError::TransactionContended,
            fault @ (PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
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

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A written document the model refuses is the client's `BadRequestData`
/// (clause 5.5.4), an unusable or forbidden `unitCode` keeping its own reason.
impl From<EntityRefusal> for TemporalError {
    fn from(refusal: EntityRefusal) -> TemporalError {
        match refusal {
            EntityRefusal::UnitCodeUnusable { attribute, source } => {
                TemporalError::Validation(TemporalValidationReason::InstanceUnitCodeUnusable { attribute, source })
            }
            EntityRefusal::UnitCodeForbidden { family, attribute } => {
                TemporalError::Validation(TemporalValidationReason::InstanceUnitCodeForbidden { family, attribute })
            }
            refused @ (EntityRefusal::Syntax { .. }
            | EntityRefusal::Malformed { .. }
            | EntityRefusal::MissingId
            | EntityRefusal::EntityId { .. }
            | EntityRefusal::TypeNotString
            | EntityRefusal::EntityType { .. }
            | EntityRefusal::MissingType
            | EntityRefusal::ExpandedAttributeName(_)
            | EntityRefusal::ExpandedMemberName(_)
            | EntityRefusal::Scope(_)
            | EntityRefusal::ScopeNull
            | EntityRefusal::EntityExpiresAt
            | EntityRefusal::DatasetId { .. }
            | EntityRefusal::InstanceId { .. }
            | EntityRefusal::DateTime { .. }
            | EntityRefusal::Datatype { .. }
            | EntityRefusal::Reference { .. }
            | EntityRefusal::DatasetIdSentinel { .. }
            | EntityRefusal::ReservedNullValue { .. }
            | EntityRefusal::NestedNullValue { .. }
            | EntityRefusal::MemberForbidden { .. }
            | EntityRefusal::UnsupportedAttributeType
            | EntityRefusal::SeveralFamilies { .. }
            | EntityRefusal::MissingValue { .. }
            | EntityRefusal::MissingObject { .. }
            | EntityRefusal::MissingFamilyValue { .. }
            | EntityRefusal::JsonNotAnObject { .. }
            | EntityRefusal::RepeatedInstance { .. }
            | EntityRefusal::MultipleDefaultInstances { .. }
            | EntityRefusal::NotAGeoProperty { .. }
            | EntityRefusal::GeoProperty { .. }
            | EntityRefusal::NullBesideData { .. }
            | EntityRefusal::NullInState { .. }
            | EntityRefusal::OutputOnlyMember { .. }
            | EntityRefusal::ReservedAttributeName { .. }
            | EntityRefusal::Proof { .. }
            | EntityRefusal::ProofOnEntity
            | EntityRefusal::PrefixedAttributeName { .. }
            | EntityRefusal::PrefixedEntityType { .. }) => TemporalError::Validation(TemporalValidationReason::Representation(refused)),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A context that does not load and a document the processor refuses are
/// JSON-LD failures, and a refused document is classified as the
/// model refuses it.
impl From<ExpansionError> for TemporalError {
    fn from(error: ExpansionError) -> TemporalError {
        match error {
            ExpansionError::Context(failure) | ExpansionError::Processor(failure) => TemporalError::JsonLd(failure),
            ExpansionError::Refused(refusal) => TemporalError::from(refusal),
            ExpansionError::PayloadBudget(exceeded) => TemporalError::PayloadBudget(exceeded),
            ExpansionError::CompactDocument(source) => TemporalError::Validation(TemporalValidationReason::CompactDocument(source)),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A request context that does not resolve is the JSON-LD failure it is; any
/// other failure to compact an entity the broker assembled itself is its own
/// fault.
impl From<CompactionError> for TemporalError {
    fn from(error: CompactionError) -> TemporalError {
        match error {
            CompactionError::JsonLd(failure) => TemporalError::JsonLd(failure),
            fault @ (CompactionError::Encode(_) | CompactionError::Refused(_)) => TemporalError::Internal(TemporalInvariant::Compaction(fault)),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A Temporal Evolution that brings an Entity into being has to name it
/// (clause 4.5.1), and a standing instance that cannot be current state is
/// refused as the model refuses it.
impl From<CurrentStateFoldError> for TemporalError {
    fn from(error: CurrentStateFoldError) -> TemporalError {
        match error {
            CurrentStateFoldError::MissingId => TemporalError::Validation(TemporalValidationReason::EntityMissingId),
            CurrentStateFoldError::MissingType => TemporalError::Validation(TemporalValidationReason::EntityMissingType),
            CurrentStateFoldError::Instance(MutationError::Refused(refusal)) => TemporalError::from(*refusal),
            CurrentStateFoldError::Instance(MutationError::LiteralMerge(source)) => TemporalError::Internal(TemporalInvariant::LiteralMerge(source)),
            CurrentStateFoldError::Instance(
                mutation @ (MutationError::AttributeNotFound { .. }
                | MutationError::DatasetNotFound { .. }
                | MutationError::DefaultInstanceNotFound { .. }
                | MutationError::AttributeTypeMismatch { .. }),
            ) => TemporalError::Validation(TemporalValidationReason::Mutation(mutation)),
        }
    }
}

impl TemporalError {
    /// Construct the refusal of an operation that counted `matched` against
    /// the `bound` configured at `maximum`.
    #[must_use]
    pub const fn too_many_results(matched: u64, maximum: u64, bound: ResultBound) -> TemporalError {
        TemporalError::TooManyResults { matched, maximum, bound }
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
            EntityError::StoreUnavailable => TemporalError::StoreUnavailable,
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
            | EntityError::PayloadBudget(_)
            | EntityError::AttributeNesting(_)
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

#[cfg(test)]
mod tests {
    use crate::error::TemporalError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(TemporalError::from(PersistenceFailure::Unavailable), TemporalError::StoreUnavailable));
        assert!(matches!(
            TemporalError::from(PersistenceFailure::Misconfigured),
            TemporalError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
