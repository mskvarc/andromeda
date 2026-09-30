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
//! Two failures are neither: a saturated pool, and a transaction PostgreSQL
//! rolled back to break a deadlock or a serialization conflict. Both leave
//! nothing behind and the identical write can run again, so each keeps a
//! category of its own; the second is what the committer re-runs.

use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_context_store::{entity::types_present::EntityTypesMissing, error::ContextStoreError};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
use andromeda_postgresql_pool::persistence_classification::classify;
use andromeda_postgresql_temporal_store::{error::TemporalStoreError, unit_code_metadata::UnitCodeMetadataError};
use andromeda_temporal::{error::TemporalError, temporal_validation_reason::TemporalValidationReason};

/// Classify a raw database failure raised while beginning or committing the
/// commit transaction.
///
/// A pool-acquire timeout at `BEGIN` or `COMMIT` becomes
/// [`CommitError::StoreSaturated`] (a 503) rather than a persistence fault.
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
        | ContextStoreError::StoredRegistrationInformation(_)
        | ContextStoreError::StatementBuild(_)
        | ContextStoreError::PayloadEncoding(_)
        | ContextStoreError::OrdinalOutOfRange(_)
        | ContextStoreError::StoredRegistration(_)
        | ContextStoreError::StoredSubscription(_)
        | ContextStoreError::StoredSnapshot(_)
        | ContextStoreError::StoredEntity(_)
        | ContextStoreError::StoredEntityMap(_)
        | ContextStoreError::CatalogueIdentityGeneration(_)
        | ContextStoreError::Invariant(_)
        | ContextStoreError::DatabaseUrlUnnamed) => CommitError::from(other.failure_class().record(&other)),
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
        TemporalError::TransactionContended => CommitError::TransactionContended,
        TemporalError::Persistence(failure) => CommitError::Persistence(failure),
        other @ (TemporalError::Validation(_)
        | TemporalError::NotFound(_)
        | TemporalError::OperationNotSupported(_)
        | TemporalError::TooManyResults { .. }
        | TemporalError::JsonLd(_)
        | TemporalError::Query(_)
        | TemporalError::Request(_)
        | TemporalError::Commit(_)
        | TemporalError::Rendering(_)
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

/// Classify a `unitCode` failure raised while projecting current state.
///
/// A code the client wrote is the client's to correct; an identity already in a
/// row that the catalog no longer registers is catalog drift (clause 4.5.2).
#[track_caller]
pub(crate) fn unit_code(error: UnitCodeMetadataError) -> CommitError {
    match error {
        UnitCodeMetadataError::Malformed => CommitError::Submission(SubmissionRefusal::UnitCodeMalformed),
        UnitCodeMetadataError::NotACommonCode | UnitCodeMetadataError::Unregistered { .. } => CommitError::Submission(SubmissionRefusal::UnitCodeUnregistered),
        drift @ (UnitCodeMetadataError::UnknownIdentity { .. } | UnitCodeMetadataError::Catalog(_)) => {
            CommitError::Persistence(PersistenceFailure::Undecodable.record(&drift))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::commit_refusal::{context_store, name_lookup, temporal, temporal_store, unit_code};
    use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_postgresql_context_store::{entity::types_present::EntityTypesMissing, error::ContextStoreError};
    use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
    use andromeda_postgresql_temporal_store::{error::TemporalStoreError, unit_code_metadata::UnitCodeMetadataError};
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
        let error = temporal(TemporalError::Persistence(PersistenceFailure::Unavailable));
        assert!(matches!(error, CommitError::Persistence(_)), "{error}");
    }

    /// Clause 4.5.2 draws `unitCode` from a closed list, so a code outside it
    /// can never be stored however often the client retries.
    #[test]
    fn an_unregistered_unit_code_is_reported_as_a_refused_submission() {
        let error = unit_code(UnitCodeMetadataError::NotACommonCode);
        assert!(matches!(error, CommitError::Submission(_)), "{error}");
        let malformed = unit_code(UnitCodeMetadataError::Malformed);
        assert!(matches!(malformed, CommitError::Submission(_)), "{malformed}");
    }

    #[test]
    fn a_unit_code_failure_inside_history_is_refused_on_the_same_terms() {
        let error = temporal_store(TemporalStoreError::UnitCode(UnitCodeMetadataError::NotACommonCode));
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

    /// A transaction the datastore rolled back is re-run by the committer, so
    /// it must reach the commit boundary as contention from every phase that
    /// can raise it. The 40P01/40001 recognition itself is exercised in
    /// andromeda-postgresql-pool, whose fake database error can carry a
    /// SQLSTATE a bare `sqlx::Error` cannot; here, the typed contention of a
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
}
