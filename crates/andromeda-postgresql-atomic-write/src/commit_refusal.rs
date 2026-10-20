//! Deciding whether a failed commit phase refused the submission or broke.
//!
//! Every phase of one atomic commit talks to a different store and fails with
//! that store's own error type, but the caller only ever needs one distinction:
//! whether the client sent something that cannot be stored, or whether the
//! broker could not do its job. Clause 5.5.9 turns the first into
//! `BadRequestData` and the second into `InternalError`, and telling a client to
//! retry a request that can never succeed is the failure this module exists to
//! prevent. The store types already carry the distinction, so it is translated
//! here once rather than re-derived — or lost — in each phase.
//!
//! Three failures are neither: a saturated pool, a transaction PostgreSQL
//! rolled back to break a deadlock or a serialization conflict, and a store
//! that could not be reached or whose connection was lost. None is known to
//! leave anything behind and the identical write can be sent again, so each
//! keeps a category of its own; only the second is what the committer re-runs,
//! because a connection lost while awaiting `COMMIT` leaves the outcome unknown
//! and only the client can decide to send the write again.

use crate::attempt_failure::{AttemptFailure, RefusedWrite};
use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_context_store::{entity::types_present::EntityTypesMissing, error::ContextStoreError};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
use andromeda_postgresql_pool::persistence_classification::classify;
use andromeda_postgresql_temporal_store::error::TemporalStoreError;
use andromeda_temporal::{error::TemporalError, temporal_validation_reason::TemporalValidationReason};

/// Classify a raw database failure raised while beginning or committing the
/// commit transaction.
///
/// A pool-acquire timeout at `BEGIN` or `COMMIT` becomes
/// [`CommitError::StoreSaturated`] and a lost connection
/// [`CommitError::StoreUnavailable`] (both a 503) rather than a persistence
/// fault.
#[track_caller]
pub(crate) fn database(error: &sqlx::Error) -> CommitError {
    CommitError::from(classify(error).record(error))
}

/// Classify a current-state store failure.
///
/// Only the missing-type refusal names the submission: clause 4.5.1 requires at
/// least one Entity type, and an Entity that carries none is a body the client
/// wrote, not a fault of the store that refused it.
#[track_caller]
pub(crate) fn context_store(error: ContextStoreError) -> CommitError {
    match error {
        ContextStoreError::EntityTypes(EntityTypesMissing::Rejected(entity)) => CommitError::Submission(SubmissionRefusal::UntypedEntity(entity)),
        ContextStoreError::EntityTypes(EntityTypesMissing::ConstraintViolated) => CommitError::Submission(SubmissionRefusal::UntypedEntityUnidentified),
        other @ (ContextStoreError::Database(_)
        | ContextStoreError::Saturated
        | ContextStoreError::Encoding(_)
        | ContextStoreError::Collation(_)
        | ContextStoreError::CopyField(_)
        | ContextStoreError::UniqueViolation(_)
        | ContextStoreError::InvalidRegex(_)
        | ContextStoreError::Render(_)
        | ContextStoreError::NameLookup(_)
        | ContextStoreError::ContextMapper(_)
        | ContextStoreError::Migration(_)
        | ContextStoreError::SchemaContract(_)
        | ContextStoreError::BranchVersion(_)
        | ContextStoreError::StatementBuild(_)
        | ContextStoreError::ChangeEventRecord(_)
        | ContextStoreError::StateRecord(_)
        | ContextStoreError::OrdinalOutOfRange(_)
        | ContextStoreError::StoredRegistration(_)
        | ContextStoreError::StoredSubscription(_)
        | ContextStoreError::StoredSnapshot(_)
        | ContextStoreError::UnencodableSnapshot(_)
        | ContextStoreError::StoredEntity(_)
        | ContextStoreError::StoredEntityMap(_)
        | ContextStoreError::StoredTenantStatus(_)
        | ContextStoreError::CatalogueIdentityGeneration(_)
        | ContextStoreError::Invariant(_)
        | ContextStoreError::DatabaseUrlUnnamed) => CommitError::from(other.failure_class().record(&other)),
    }
}

/// Classify a failure of the fused entity statements.
///
/// The statements write every entity of the commit at once, but the store names
/// the entity it refused for carrying no type (clause 4.5.1), so that refusal is
/// attributed to the one write it came from.
#[track_caller]
pub(crate) fn entity_statement(error: ContextStoreError) -> AttemptFailure {
    match error {
        ContextStoreError::EntityTypes(EntityTypesMissing::Rejected(entity)) => AttemptFailure::Refused(Box::new(RefusedWrite {
            refusal: SubmissionRefusal::UntypedEntity(entity.clone()), // clone: the refusal and its attribution each own the entity.
            entity_id: entity,
        })),
        other @ (ContextStoreError::EntityTypes(EntityTypesMissing::ConstraintViolated)
        | ContextStoreError::Database(_)
        | ContextStoreError::Saturated
        | ContextStoreError::Encoding(_)
        | ContextStoreError::Collation(_)
        | ContextStoreError::CopyField(_)
        | ContextStoreError::UniqueViolation(_)
        | ContextStoreError::InvalidRegex(_)
        | ContextStoreError::Render(_)
        | ContextStoreError::NameLookup(_)
        | ContextStoreError::ContextMapper(_)
        | ContextStoreError::Migration(_)
        | ContextStoreError::SchemaContract(_)
        | ContextStoreError::BranchVersion(_)
        | ContextStoreError::StatementBuild(_)
        | ContextStoreError::ChangeEventRecord(_)
        | ContextStoreError::StateRecord(_)
        | ContextStoreError::OrdinalOutOfRange(_)
        | ContextStoreError::StoredRegistration(_)
        | ContextStoreError::StoredSubscription(_)
        | ContextStoreError::StoredSnapshot(_)
        | ContextStoreError::UnencodableSnapshot(_)
        | ContextStoreError::StoredEntity(_)
        | ContextStoreError::StoredEntityMap(_)
        | ContextStoreError::StoredTenantStatus(_)
        | ContextStoreError::CatalogueIdentityGeneration(_)
        | ContextStoreError::Invariant(_)
        | ContextStoreError::DatabaseUrlUnnamed) => AttemptFailure::Failed(context_store(other)),
    }
}

/// Classify a temporal-policy failure raised inside the commit transaction.
///
/// The temporal layer already separates what the request said from what the
/// store could do, so a refused submission is exactly the commit's refusal.
#[track_caller]
pub(crate) fn temporal(error: TemporalError) -> CommitError {
    match error {
        TemporalError::Validation(TemporalValidationReason::Refused(refusal)) => CommitError::Submission(refusal),
        // Pool saturation is backpressure the client can retry, not a fault.
        TemporalError::StoreSaturated => CommitError::StoreSaturated,
        TemporalError::StoreUnavailable => CommitError::StoreUnavailable,
        TemporalError::TransactionContended => CommitError::TransactionContended,
        TemporalError::Persistence(failure) => CommitError::Persistence(failure),
        other @ (TemporalError::Validation(_)
        | TemporalError::NotFound(_)
        | TemporalError::OperationNotSupported(_)
        | TemporalError::TooManyResults { .. }
        | TemporalError::PayloadBudget(_)
        | TemporalError::AttributeNesting(_)
        | TemporalError::JsonLd(_)
        | TemporalError::Query(_)
        | TemporalError::Request(_)
        | TemporalError::Commit(_)
        | TemporalError::HistoryUnsettled(_)
        | TemporalError::Internal(_)) => CommitError::Persistence(PersistenceFailure::ContractBroken.record(&other)),
    }
}

/// Classify a temporal-store failure raised while writing history.
///
/// The store's own conversion already decides which of its variants describe
/// the written content, so the decision is taken there once and read here.
#[track_caller]
pub(crate) fn temporal_store(error: TemporalStoreError) -> CommitError {
    temporal(TemporalError::from(error))
}

/// Classify a failure of the native history phase.
///
/// The store records every landed write's intent in one chunk, and names the
/// entity whose intent failed; a refusal among those failures is attributed to
/// that entity's write.
#[track_caller]
pub(crate) fn history(error: TemporalStoreError) -> AttemptFailure {
    match error {
        TemporalStoreError::EntityIntent { entity_id, source } => AttemptFailure::of_write(temporal_store(*source), entity_id),
        other @ (TemporalStoreError::Database(_)
        | TemporalStoreError::StoredHistory(_)
        | TemporalStoreError::StoredJson(_)
        | TemporalStoreError::StoredCatalog(_)
        | TemporalStoreError::StoredAggregation(_)
        | TemporalStoreError::Render(_)
        | TemporalStoreError::NotFound(_)
        | TemporalStoreError::Encoding(_)
        | TemporalStoreError::Collation(_)
        | TemporalStoreError::HistoryMembers(_)
        | TemporalStoreError::NameLookup(_)
        | TemporalStoreError::Invariant(_)
        | TemporalStoreError::Refused(_)
        | TemporalStoreError::WriteOrdinal(_)
        | TemporalStoreError::StatementBuild(_)
        | TemporalStoreError::RetentionHorizon(_)
        | TemporalStoreError::ColumnstoreHorizon(_)
        | TemporalStoreError::StoredTime(_)
        | TemporalStoreError::ScopeHasNoAttributeRow
        | TemporalStoreError::TimestampPrecision(_)
        | TemporalStoreError::MixedAggregationGroup(_)
        | TemporalStoreError::DuplicateInstanceClaim(_)
        | TemporalStoreError::WriteRecordMismatch { .. }
        | TemporalStoreError::ColumnstoreSettingsDrift { .. }) => AttemptFailure::Failed(temporal_store(other)),
    }
}

/// Classify a name-dictionary failure raised while minting the keys a write
/// binds.
///
/// Minting inserts into a tenant-global dictionary, so it is where two
/// concurrent writers of overlapping new names meet; a transaction rolled back
/// there is contention like anywhere else in the commit.
#[track_caller]
pub(crate) fn name_lookup(error: &NameLookupError) -> CommitError {
    CommitError::from(error.failure_class().record(error))
}

#[cfg(test)]
mod tests {
    use crate::{
        attempt_failure::AttemptFailure,
        commit_refusal::{context_store, entity_statement, history, name_lookup, temporal, temporal_store},
    };
    use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_postgresql_context_store::{entity::types_present::EntityTypesMissing, error::ContextStoreError};
    use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
    use andromeda_postgresql_temporal_store::{duplicate_instance_claim::DuplicateInstanceClaimError, error::TemporalStoreError};
    use andromeda_temporal::error::TemporalError;

    fn entity_id() -> Identifier {
        "urn:ngsi-ld:Building:1".parse().expect("valid entity URI")
    }

    /// Clause 4.5.1: an Entity with no type is a body the client wrote, so the
    /// refusal must not tell that client to retry.
    #[test]
    fn a_typeless_entity_is_reported_as_a_refused_submission() {
        let named = context_store(ContextStoreError::EntityTypes(EntityTypesMissing::Rejected(entity_id())));
        assert!(matches!(named, CommitError::Submission(_)), "{named}");
        let anonymous = context_store(ContextStoreError::EntityTypes(EntityTypesMissing::ConstraintViolated));
        assert!(matches!(anonymous, CommitError::Submission(_)), "{anonymous}");
    }

    #[test]
    fn a_failed_statement_remains_the_brokers_own_fault() {
        let error = context_store(ContextStoreError::Database(sqlx::Error::RowNotFound));
        assert!(matches!(error, CommitError::Persistence(_)), "{error}");
    }

    #[test]
    fn an_invalid_temporal_operation_is_reported_as_a_refused_submission() {
        let error = temporal(TemporalError::from(SubmissionRefusal::PropertyMissingValue));
        assert!(matches!(error, CommitError::Submission(_)), "{error}");
    }

    #[test]
    fn a_temporal_persistence_fault_stays_a_persistence_failure() {
        let error = temporal(TemporalError::Persistence(PersistenceFailure::StatementRejected));
        assert!(matches!(error, CommitError::Persistence(_)), "{error}");
    }

    #[test]
    fn a_refusal_inside_history_is_refused_on_the_same_terms() {
        let error = temporal_store(TemporalStoreError::Refused(SubmissionRefusal::RelationshipMissingObject));
        assert!(matches!(error, CommitError::Submission(_)), "{error}");
    }

    #[test]
    fn a_history_decode_failure_stays_a_persistence_failure() {
        let error = temporal_store(TemporalStoreError::ScopeHasNoAttributeRow);
        assert!(matches!(error, CommitError::Persistence(_)), "{error}");
    }

    /// A pool-acquire timeout at `BEGIN` or `COMMIT` is backpressure the client
    /// can retry, so the commit boundary reports it as `StoreSaturated` (a 503),
    /// never a persistence fault.
    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        use crate::commit_refusal::database;
        assert!(matches!(database(&sqlx::Error::PoolTimedOut), CommitError::StoreSaturated));
        assert!(matches!(database(&sqlx::Error::RowNotFound), CommitError::Persistence(_)));
        assert!(matches!(
            name_lookup(&NameLookupError::from(sqlx::Error::PoolTimedOut)),
            CommitError::StoreSaturated
        ));
    }

    /// A connection lost under the commit, or a store that cannot be reached,
    /// is a retryable 503 from every phase that can meet it, never the
    /// persistence fault a broker malfunction earns.
    #[test]
    fn a_store_lost_mid_commit_is_unavailable() {
        use crate::commit_refusal::database;
        use std::io::{Error as IoError, ErrorKind};
        assert!(matches!(
            database(&sqlx::Error::Io(IoError::from(ErrorKind::ConnectionReset))),
            CommitError::StoreUnavailable
        ));
        assert!(matches!(database(&sqlx::Error::PoolClosed), CommitError::StoreUnavailable));
        assert!(matches!(temporal(TemporalError::StoreUnavailable), CommitError::StoreUnavailable));
        assert!(matches!(
            name_lookup(&NameLookupError::Statement(sqlx::Error::Io(IoError::from(ErrorKind::UnexpectedEof)))),
            CommitError::StoreUnavailable
        ));
    }

    /// A transaction the datastore rolled back is re-run by the committer, so
    /// it must reach the commit boundary as contention from every phase that
    /// can raise it. The 40P01/40001 recognition itself is exercised where a
    /// fake database error can carry a SQLSTATE a bare `sqlx::Error` cannot;
    /// here, the typed contention of a
    /// lower layer is carried through and nothing else is mistaken for it.
    #[test]
    fn contention_from_a_lower_layer_stays_contention_and_nothing_else_becomes_it() {
        assert!(matches!(temporal(TemporalError::TransactionContended), CommitError::TransactionContended));
        assert!(matches!(temporal(TemporalError::StoreSaturated), CommitError::StoreSaturated));
        assert!(matches!(
            context_store(ContextStoreError::Database(sqlx::Error::RowNotFound)),
            CommitError::Persistence(_)
        ));
        assert!(matches!(
            name_lookup(&NameLookupError::Statement(sqlx::Error::RowNotFound)),
            CommitError::Persistence(_)
        ));
    }

    /// Clause 4.5.1: the entity statement names the entity that carried no
    /// type, so the rest of the commit need not answer for it.
    #[test]
    fn a_typeless_entity_refused_by_the_entity_statement_is_attributed_to_its_write() {
        let failure = entity_statement(ContextStoreError::EntityTypes(EntityTypesMissing::Rejected(entity_id())));
        assert!(
            matches!(&failure, AttemptFailure::Refused(refused) if refused.entity_id == entity_id() && matches!(refused.refusal, SubmissionRefusal::UntypedEntity(_))),
            "{failure:?}"
        );
        let anonymous = entity_statement(ContextStoreError::EntityTypes(EntityTypesMissing::ConstraintViolated));
        assert!(matches!(anonymous, AttemptFailure::Failed(CommitError::Submission(_))), "{anonymous:?}");
    }

    #[test]
    fn a_failed_entity_statement_fails_the_attempt_as_a_whole() {
        let failure = entity_statement(ContextStoreError::Database(sqlx::Error::RowNotFound));
        assert!(matches!(failure, AttemptFailure::Failed(CommitError::Persistence(_))), "{failure:?}");
    }

    /// A refusal raised while recording one entity's intent is that entity's;
    /// a fault raised there is the store's, whichever intent it struck.
    #[test]
    fn a_refused_history_intent_is_attributed_to_the_entity_it_names() {
        let entity_intent = |source: TemporalStoreError| TemporalStoreError::EntityIntent {
            entity_id: entity_id(),
            source: Box::new(source),
        };
        let refused = history(entity_intent(TemporalStoreError::Refused(SubmissionRefusal::RelationshipMissingObject)));
        assert!(
            matches!(&refused, AttemptFailure::Refused(refused) if refused.entity_id == entity_id() && matches!(refused.refusal, SubmissionRefusal::RelationshipMissingObject)),
            "{refused:?}"
        );
        let fault = history(entity_intent(TemporalStoreError::ScopeHasNoAttributeRow));
        assert!(matches!(fault, AttemptFailure::Failed(CommitError::Persistence(_))), "{fault:?}");
        let unattributed = history(TemporalStoreError::DuplicateInstanceClaim(DuplicateInstanceClaimError));
        assert!(matches!(unattributed, AttemptFailure::Failed(CommitError::Submission(_))), "{unattributed:?}");
    }
}
