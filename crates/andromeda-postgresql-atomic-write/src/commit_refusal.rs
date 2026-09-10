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

use andromeda_context_commit::error::CommitError;
use andromeda_postgresql_context_store::error::ContextStoreError;
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

/// Classify a current-state store failure.
///
/// Only the missing-type refusal names the submission: clause 4.5.1 requires at
/// least one Entity type, and an Entity that carries none is a body the client
/// wrote, not a fault of the store that refused it.
pub(crate) fn context_store(error: ContextStoreError) -> CommitError {
    match error {
        ContextStoreError::EntityTypes(detail) => CommitError::Submission { detail: detail.to_string() },
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
        | ContextStoreError::Internal(_) => persistence(error),
    }
}

/// Classify a temporal-policy failure raised inside the commit transaction.
///
/// The temporal layer already separates what the request said from what the
/// store could do, so the validation variant is exactly the refused submission.
pub(crate) fn temporal(error: TemporalError) -> CommitError {
    match error {
        TemporalError::Validation { detail } => CommitError::Submission { detail },
        TemporalError::NotFound { .. }
        | TemporalError::OperationNotSupported { .. }
        | TemporalError::JsonLd(_)
        | TemporalError::Query(_)
        | TemporalError::Request(_)
        | TemporalError::Commit(_)
        | TemporalError::Persistence { .. }
        | TemporalError::Rendering { .. }
        | TemporalError::Internal { .. } => persistence(error),
    }
}

/// Classify a temporal-store failure raised while writing history.
///
/// The store's own conversion already decides which of its variants describe
/// the written content, so the decision is taken there once and read here.
pub(crate) fn temporal_store(error: TemporalStoreError) -> CommitError {
    temporal(TemporalError::from(error))
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
    use crate::commit_refusal::{context_store, persistence, temporal, temporal_store, unit_code};
    use andromeda_context_commit::error::CommitError;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_postgresql_context_store::{entity::types_present::EntityTypesMissing, error::ContextStoreError};
    use andromeda_postgresql_temporal_store::{error::TemporalStoreError, unit_code_metadata::UnitCodeMetadataError};
    use andromeda_temporal::error::TemporalError;

    fn entity_id() -> Uri {
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
        let error = temporal(TemporalError::validation("Property missing value"));
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
}
