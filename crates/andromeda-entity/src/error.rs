//! Failures produced by entity policy and its persistence ports.

use crate::{entity_invariant::EntityInvariant, entity_operation_unsupported::EntityOperationUnsupported, entity_validation_reason::EntityValidationReason};
use andromeda_context_commit::error::CommitError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_compaction::compaction_error::CompactionError;
use andromeda_ngsi_ld_entity::{operation::mutation_error::MutationError, refusal::EntityRefusal};
use andromeda_ngsi_ld_expansion::error::ExpansionError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_representation::{attribute_nesting::AttributeNestingExceeded, payload_budget::PayloadBudgetExceeded};
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use derive_more::IsVariant;
use thiserror::Error;

/// A failure while validating, reading, querying, mutating, or persisting an entity.
#[derive(Debug, Error, IsVariant)]
pub enum EntityError {
    /// The entity payload or operation is invalid.
    #[error("invalid entity operation: {0}")]
    Validation(#[from] EntityValidationReason),
    /// The requested entity does not exist.
    #[error("entity {identifier} not found")]
    NotFound {
        /// Missing entity identifier.
        identifier: Identifier,
    },
    /// The requested entity type does not exist.
    #[error("entity type {name} not found")]
    TypeNotFound {
        /// Missing entity type name.
        name: EntityTypeName,
    },
    /// The requested attribute does not exist.
    #[error("attribute {name} not found")]
    AttributeNotFound {
        /// Missing attribute name.
        name: AttributeName,
    },
    /// A requested entity member or dataset instance does not exist.
    #[error("entity member not found: {0}")]
    MemberNotFound(MutationError),
    /// An entity already uses the requested identifier.
    #[error("entity {identifier} already exists")]
    AlreadyExists {
        /// Conflicting entity identifier.
        identifier: Identifier,
    },
    /// Concurrent writers superseded the mutation on every attempt it was given.
    ///
    /// The submitted content is not at fault and nothing about it needs
    /// changing: the entity is simply being written faster than a
    /// read-modify-write can land against it.
    #[error("entity {identifier} is being modified concurrently: the write lost its precondition on all {attempts} attempts and can be retried unchanged")]
    Contention {
        /// Contended entity identifier.
        identifier: Identifier,
        /// Attempts spent before the broker gave up.
        attempts: u32,
    },
    /// The selected storage capability cannot answer the operation safely.
    #[error("entity operation is not supported: {0}")]
    OperationNotSupported(EntityOperationUnsupported),
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
    /// JSON representation processing failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// An atomic context write failed for a reason the client cannot act on.
    ///
    /// A commit that refused the submitted content never reaches this variant:
    /// [`From<CommitError>`] turns that refusal into [`EntityError::Validation`],
    /// which is what clause 5.5.6 reports as `BadRequestData`.
    #[error(transparent)]
    Commit(CommitError),
    /// A persistence adapter could not complete the operation.
    #[error("entity persistence failed: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// The datastore could not be reached, or the connection to it was lost.
    ///
    /// Not a fault of the request: nothing of it is known to have persisted, so
    /// the identical request can be sent again once the store is back. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore is temporarily unavailable; retry shortly")]
    StoreUnavailable,
    /// The datastore rolled the write back to break a deadlock or a
    /// serialization conflict with a concurrent transaction, on every attempt
    /// the committer made.
    ///
    /// Not a fault: nothing of the write persisted, so the identical request
    /// can be sent again. It is answered 503 with `Retry-After`, never the 500
    /// a broker malfunction earns.
    #[error("the write was rolled back by a concurrent transaction; retry shortly")]
    TransactionContended,
    /// An internal entity invariant could not be satisfied.
    #[error("entity invariant failed: {0}")]
    Internal(#[from] EntityInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// The commit port reports a refused submission separately from its own faults,
/// and collapsing both into one variant would answer an unusable request with
/// `InternalError` and invite the client to retry it forever (clause 5.5.6).
impl From<CommitError> for EntityError {
    fn from(error: CommitError) -> EntityError {
        match error {
            CommitError::Submission(refusal) => EntityError::Validation(EntityValidationReason::CommitRefused(refusal)),
            // Saturation is backpressure the client can retry, so it keeps its
            // own category rather than being folded into the internal-fault
            // `Commit` variant.
            CommitError::StoreSaturated => EntityError::StoreSaturated,
            CommitError::TransactionContended => EntityError::TransactionContended,
            CommitError::StoreUnavailable => EntityError::StoreUnavailable,
            failure @ (CommitError::DuplicateEntity { .. }
            | CommitError::Persistence(_)
            | CommitError::Invariant(_)
            | CommitError::ReplayLeaseLost { .. }
            | CommitError::UnexpectedOutcomeCount { .. }) => EntityError::Commit(failure),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store, a transaction rolled back under contention and a store
/// that cannot be reached are conditions the client can retry, so each keeps
/// its own retryable category; only a fault becomes
/// [`EntityError::Persistence`].
impl From<PersistenceFailure> for EntityError {
    fn from(failure: PersistenceFailure) -> EntityError {
        match failure {
            PersistenceFailure::Saturated => EntityError::StoreSaturated,
            PersistenceFailure::Unavailable => EntityError::StoreUnavailable,
            PersistenceFailure::RolledBack => EntityError::TransactionContended,
            fault @ (PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
            | PersistenceFailure::SchemaMismatch
            | PersistenceFailure::ContractBroken) => EntityError::Persistence(fault),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A written document the model refuses is the client's `BadRequestData`
/// (clause 5.5.4).
impl From<EntityRefusal> for EntityError {
    fn from(refusal: EntityRefusal) -> EntityError {
        match refusal {
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
            | EntityRefusal::UnitCodeForbidden { .. }
            | EntityRefusal::JsonNotAnObject { .. }
            | EntityRefusal::UnitCodeUnusable { .. }
            | EntityRefusal::RepeatedInstance { .. }
            | EntityRefusal::MultipleDefaultInstances { .. }
            | EntityRefusal::GeoProperty { .. }
            | EntityRefusal::NullBesideData { .. }
            | EntityRefusal::NullInState { .. }
            | EntityRefusal::OutputOnlyMember { .. }
            | EntityRefusal::ReservedAttributeName { .. }) => EntityError::Validation(EntityValidationReason::Refused(Box::new(refused))),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// Clause 5.5.2 makes a well-formed request naming an attribute or instance
/// the entity does not have `ResourceNotFound`, and a fragment that cannot be
/// applied to what the entity holds `BadRequestData`.
impl From<MutationError> for EntityError {
    fn from(error: MutationError) -> EntityError {
        match error {
            missing @ (MutationError::AttributeNotFound { .. } | MutationError::DatasetNotFound { .. } | MutationError::DefaultInstanceNotFound { .. }) => {
                EntityError::MemberNotFound(missing)
            }
            MutationError::Refused(refusal) => EntityError::from(*refusal),
            mismatch @ MutationError::AttributeTypeMismatch { .. } => EntityError::Validation(EntityValidationReason::Mutation(mismatch)),
            MutationError::LiteralMerge(source) => EntityError::Internal(EntityInvariant::LiteralMerge(source)),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A context that does not load and a document the processor refuses are
/// JSON-LD failures; a refused document is classified as the model refuses it.
impl From<ExpansionError> for EntityError {
    fn from(error: ExpansionError) -> EntityError {
        match error {
            ExpansionError::Context(failure) | ExpansionError::Processor(failure) => EntityError::JsonLd(failure),
            ExpansionError::Refused(refusal) => EntityError::from(refusal),
            ExpansionError::CompactDocument(source) => EntityError::Validation(EntityValidationReason::CompactDocument(source)),
        }
    }
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A stored entity that cannot be written for the processor, or does not read
/// back once compacted, is the broker's fault.
impl From<CompactionError> for EntityError {
    fn from(error: CompactionError) -> EntityError {
        match error {
            CompactionError::JsonLd(failure) => EntityError::JsonLd(failure),
            CompactionError::Encode(source) => EntityError::Internal(EntityInvariant::StateEncoding(source)),
            CompactionError::Refused(refused) => EntityError::Internal(EntityInvariant::CompactReading(refused)),
        }
    }
}

impl EntityError {
    /// Construct a missing-entity failure.
    #[must_use]
    pub fn not_found(identifier: &Identifier) -> EntityError {
        EntityError::NotFound {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct a missing-entity-type failure.
    #[must_use]
    pub fn type_not_found(name: &EntityTypeName) -> EntityError {
        EntityError::TypeNotFound {
            name: name.clone(), // clone: the error owns the borrowed request type.
        }
    }

    /// Construct a missing-attribute failure.
    #[must_use]
    pub fn attribute_not_found(name: &AttributeName) -> EntityError {
        EntityError::AttributeNotFound {
            name: name.clone(), // clone: the error owns the borrowed request attribute.
        }
    }

    /// Construct an identifier-conflict failure.
    #[must_use]
    pub fn already_exists(identifier: &Identifier) -> EntityError {
        EntityError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct a write-contention failure over the attempts it consumed.
    #[must_use]
    pub fn contention(identifier: &Identifier, attempts: u32) -> EntityError {
        EntityError::Contention {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
            attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_validation_reason::EntityValidationReason, error::EntityError};
    use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_ngsi_ld_entity::{operation::mutation_error::MutationError, refusal::EntityRefusal};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    fn speed() -> AttributeName {
        "https://example.org/speed".parse().expect("a valid attribute name")
    }

    /// Clause 5.5.4: a document the model refuses is the client's
    /// `BadRequestData`, carried typed with its own message.
    #[test]
    fn a_refused_document_is_a_validation_failure_carrying_the_refusal() {
        let error = EntityError::from(EntityRefusal::MissingType);
        assert!(
            matches!(&error, EntityError::Validation(EntityValidationReason::Refused(refusal)) if matches!(**refusal, EntityRefusal::MissingType)),
            "{error:?}"
        );
        assert!(error.to_string().starts_with("invalid entity operation: "), "{error}");
    }

    /// Clause 5.5.2: a well-formed request naming a member the entity lacks is
    /// `ResourceNotFound`, a fragment of another family `BadRequestData`, and a
    /// written instance that cannot become state is classified as its refusal.
    #[test]
    fn a_mutation_failure_is_classified_by_what_it_names() {
        let missing = EntityError::from(MutationError::AttributeNotFound { attribute: speed() });
        assert!(
            matches!(missing, EntityError::MemberNotFound(MutationError::AttributeNotFound { .. })),
            "{missing:?}"
        );
        let mismatch = EntityError::from(MutationError::AttributeTypeMismatch {
            attribute: speed(),
            fragment_type: AttributeType::Relationship,
            instance_type: AttributeType::Property,
        });
        assert!(matches!(mismatch, EntityError::Validation(EntityValidationReason::Mutation(_))), "{mismatch:?}");
        let refused = EntityError::from(MutationError::from(EntityRefusal::MissingId));
        assert!(
            matches!(&refused, EntityError::Validation(EntityValidationReason::Refused(refusal)) if matches!(**refusal, EntityRefusal::MissingId)),
            "{refused:?}"
        );
    }

    #[test]
    fn backpressure_keeps_its_retryable_categories() {
        assert!(matches!(EntityError::from(PersistenceFailure::Saturated), EntityError::StoreSaturated));
        assert!(matches!(EntityError::from(PersistenceFailure::RolledBack), EntityError::TransactionContended));
        assert!(matches!(
            EntityError::from(PersistenceFailure::Undecodable),
            EntityError::Persistence(PersistenceFailure::Undecodable)
        ));
    }

    #[test]
    fn a_refused_commit_is_a_validation_failure_carrying_the_typed_refusal() {
        let error = EntityError::from(CommitError::Submission(SubmissionRefusal::RelationshipMissingObject));
        assert!(matches!(
            error,
            EntityError::Validation(EntityValidationReason::CommitRefused(SubmissionRefusal::RelationshipMissingObject))
        ));
    }

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(EntityError::from(PersistenceFailure::Unavailable), EntityError::StoreUnavailable));
        assert!(matches!(
            EntityError::from(PersistenceFailure::Misconfigured),
            EntityError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }

    #[test]
    fn an_unreachable_store_at_commit_stays_retryable() {
        assert!(matches!(EntityError::from(CommitError::StoreUnavailable), EntityError::StoreUnavailable));
    }
}
