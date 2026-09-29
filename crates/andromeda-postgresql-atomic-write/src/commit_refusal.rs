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

use andromeda_context_commit::error::CommitError;
use andromeda_postgresql_context_store::error::ContextStoreError;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
use andromeda_postgresql_pool::is_transaction_rolled_back;
use andromeda_postgresql_temporal_store::{error::TemporalStoreError, unit_code_metadata::UnitCodeMetadataError};
use andromeda_temporal::error::TemporalError;
use std::fmt::Display;

/// Convert a concrete store failure that is the broker's own fault.
///
/// Reserved for failures no request content can explain: a lost connection, a
/// statement the adapter built wrong, an encoding the store could not produce.
pub(crate) fn persistence(error: impl Display) -> CommitError {
    CommitError::Persistence { detail: error.to_string() }
}

/// Classify a raw database failure raised while beginning or committing the
/// commit transaction.
///
/// Reuses the context store's saturation detection so a pool-acquire timeout at
/// `BEGIN` or `COMMIT` becomes [`CommitError::StoreSaturated`] (a 503) rather
/// than a persistence fault.
pub(crate) fn database(error: sqlx::Error) -> CommitError {
    context_store(ContextStoreError::from(error))
}

/// Classify a current-state store failure.
///
/// Only the missing-type refusal names the submission: clause 4.5.1 requires at
/// least one Entity type, and an Entity that carries none is a body the client
/// wrote, not a fault of the store that refused it.
pub(crate) fn context_store(error: ContextStoreError) -> CommitError {
    if error.is_transaction_rolled_back() {
        return CommitError::TransactionContended;
    }
    match error {
        ContextStoreError::EntityTypes(detail) => CommitError::Submission { detail: detail.to_string() },
        // Pool saturation is backpressure the client can retry, not a fault.
        ContextStoreError::Saturated => CommitError::StoreSaturated,
        ContextStoreError::Database(_)
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
        | ContextStoreError::Internal(_) => persistence(error),
    }
}

/// Classify a temporal-policy failure raised inside the commit transaction.
///
/// The temporal layer already separates what the request said from what the
/// store could do, so the validation variant is exactly the refused submission.
pub(crate) fn temporal(error: TemporalError) -> CommitError {
    match error {
        // The refused submission carries the typed validation reason; relay its
        // rendered text, as the commit port cannot hold the reason typed without
        // a dependency cycle.
        TemporalError::Validation(reason) => CommitError::Submission { detail: reason.to_string() },
        // Pool saturation is backpressure the client can retry, not a fault.
        TemporalError::StoreSaturated => CommitError::StoreSaturated,
        TemporalError::TransactionContended => CommitError::TransactionContended,
        TemporalError::NotFound(_)
        | TemporalError::OperationNotSupported(_)
        | TemporalError::TooManyResults { .. }
        | TemporalError::JsonLd(_)
        | TemporalError::Query(_)
        | TemporalError::Request(_)
        | TemporalError::Commit(_)
        | TemporalError::Persistence(_)
        | TemporalError::Rendering(_)
        | TemporalError::Internal(_) => persistence(error),
    }
}

/// Classify a temporal-store failure raised while writing history.
///
/// The store's own conversion already decides which of its variants describe
/// the written content, so the decision is taken there once and read here.
pub(crate) fn temporal_store(error: TemporalStoreError) -> CommitError {
    temporal(TemporalError::from(error))
}

/// Classify a name-dictionary failure raised while minting the keys a write
/// binds.
///
/// Minting inserts into a tenant-global dictionary, so it is where two
/// concurrent writers of overlapping new names meet; a transaction rolled back
/// there is contention like anywhere else in the commit.
pub(crate) fn name_lookup(error: NameLookupError) -> CommitError {
    match &error {
        // Pool saturation is backpressure the client can retry, not a fault.
        NameLookupError::Saturated => CommitError::StoreSaturated,
        NameLookupError::Statement(inner) if is_transaction_rolled_back(inner) => CommitError::TransactionContended,
        NameLookupError::Statement(_)
        | NameLookupError::StatementBuild(_)
        | NameLookupError::StoredAttributeName { .. }
        | NameLookupError::StoredEntityTypeName { .. }
        | NameLookupError::Growth(_) => persistence(error),
    }
}

/// Classify a `unitCode` failure raised while projecting current state.
///
/// A code the client wrote is the client's to correct; an identity already in a
/// row that the catalog no longer registers is catalog drift (clause 4.5.2).
pub(crate) fn unit_code(error: UnitCodeMetadataError) -> CommitError {
    match error {
        UnitCodeMetadataError::Malformed | UnitCodeMetadataError::Unregistered { .. } => CommitError::Submission { detail: error.to_string() },
        UnitCodeMetadataError::UnknownIdentity { .. } | UnitCodeMetadataError::Catalog(_) => persistence(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::commit_refusal::{context_store, name_lookup, persistence, temporal, temporal_store, unit_code};
    use andromeda_context_commit::error::CommitError;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_context_store::{entity::types_present::EntityTypesMissing, error::ContextStoreError};
    use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
    use andromeda_postgresql_temporal_store::{error::TemporalStoreError, unit_code_metadata::UnitCodeMetadataError};
    use andromeda_temporal::{error::TemporalError, temporal_validation_reason::TemporalValidationReason};

    fn entity_id() -> Identifier {
        "urn:ngsi-ld:Building:1".parse().expect("valid entity URI")
    }

    #[test]
    fn concrete_store_errors_are_erased_at_the_port_boundary() {
        assert!(matches!(persistence("database unavailable"), CommitError::Persistence { .. }));
    }

    /// Clause 4.5.1: an Entity with no type is a body the client wrote, so the
    /// refusal must not tell that client to retry.
    #[test]
    fn a_typeless_entity_is_reported_as_a_refused_submission() {
        let named = context_store(ContextStoreError::EntityTypes(EntityTypesMissing::Rejected(entity_id())));
        assert!(matches!(named, CommitError::Submission { .. }), "{named}");
        let anonymous = context_store(ContextStoreError::EntityTypes(EntityTypesMissing::ConstraintViolated));
        assert!(matches!(anonymous, CommitError::Submission { .. }), "{anonymous}");
    }

    #[test]
    fn a_failed_statement_remains_the_brokers_own_fault() {
        let error = context_store(ContextStoreError::Internal("malformed stored row".to_string()));
        assert!(matches!(error, CommitError::Persistence { .. }), "{error}");
    }

    #[test]
    fn an_invalid_temporal_operation_is_reported_as_a_refused_submission() {
        let error = temporal(TemporalError::Validation(TemporalValidationReason::PropertyMissingValue));
        assert!(matches!(error, CommitError::Submission { .. }), "{error}");
    }

    #[test]
    fn a_temporal_persistence_fault_stays_a_persistence_failure() {
        let error = temporal(TemporalError::persistence("connection reset"));
        assert!(matches!(error, CommitError::Persistence { .. }), "{error}");
    }

    /// Clause 4.5.2 draws `unitCode` from a closed list, so a code outside it
    /// can never be stored however often the client retries.
    #[test]
    fn an_unregistered_unit_code_is_reported_as_a_refused_submission() {
        let error = unit_code(UnitCodeMetadataError::Unregistered { code: "QQQ".to_string() });
        assert!(matches!(error, CommitError::Submission { .. }), "{error}");
        let malformed = unit_code(UnitCodeMetadataError::Malformed);
        assert!(matches!(malformed, CommitError::Submission { .. }), "{malformed}");
    }

    #[test]
    fn a_unit_code_failure_inside_history_is_refused_on_the_same_terms() {
        let error = temporal_store(TemporalStoreError::UnitCode(UnitCodeMetadataError::Unregistered { code: "QQQ".to_string() }));
        assert!(matches!(error, CommitError::Submission { .. }), "{error}");
    }

    #[test]
    fn a_history_decode_failure_stays_a_persistence_failure() {
        let error = temporal_store(TemporalStoreError::Decode("unreadable column".to_string()));
        assert!(matches!(error, CommitError::Persistence { .. }), "{error}");
    }

    /// A pool-acquire timeout at `BEGIN` or `COMMIT` is backpressure the client
    /// can retry, so the commit boundary reports it as `StoreSaturated` (a 503),
    /// never a persistence fault.
    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        use crate::commit_refusal::database;
        assert!(matches!(database(sqlx::Error::PoolTimedOut), CommitError::StoreSaturated));
        assert!(matches!(database(sqlx::Error::RowNotFound), CommitError::Persistence { .. }));
        assert!(matches!(
            name_lookup(NameLookupError::from(sqlx::Error::PoolTimedOut)),
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
            CommitError::Persistence { .. }
        ));
        assert!(matches!(
            name_lookup(NameLookupError::Statement(sqlx::Error::RowNotFound)),
            CommitError::Persistence { .. }
        ));
    }
}
