//! How a temporal sweep's store failure is classified for its supervisor.
//!
//! Retention and columnstore conversion run as scheduled tasks over the same
//! temporal store; a database failure is worth the next run, anything else is
//! a broken invariant or a refusal no rerun changes.

use andromeda_postgresql_temporal_store::error::TemporalStoreError;
use andromeda_task_runtime::failure::{FailureClass, TaskFailure};

/// Classify a temporal sweep's store failure at the composition boundary.
pub(super) const fn classify_sweep_failure(error: TemporalStoreError) -> TaskFailure<TemporalStoreError> {
    let class = match &error {
        // A collation object is created on demand by a read that orders under
        // it (clause 4.23.1); the statement fails for the same reasons any
        // other one does, so a retry is worth taking.
        TemporalStoreError::Database(_) | TemporalStoreError::NameLookup(_) | TemporalStoreError::Collation(_) => FailureClass::Transient,
        TemporalStoreError::WriteOrdinal(_)
        | TemporalStoreError::StatementBuild(_)
        | TemporalStoreError::RetentionHorizon(_)
        | TemporalStoreError::ColumnstoreHorizon(_)
        | TemporalStoreError::StoredJson(_)
        | TemporalStoreError::ScopeHasNoAttributeRow
        | TemporalStoreError::StoredHistory(_)
        | TemporalStoreError::StoredCatalog(_)
        | TemporalStoreError::StoredAggregation(_)
        | TemporalStoreError::Render(_)
        | TemporalStoreError::Encoding(_)
        | TemporalStoreError::HistoryMembers(_)
        | TemporalStoreError::Invariant(_)
        | TemporalStoreError::MixedAggregationGroup(_)
        | TemporalStoreError::WriteRecordMismatch { .. }
        | TemporalStoreError::ColumnstoreSettingsDrift { .. }
        // A sweep only removes or converts stored history and records no
        // intent, so a failure naming the entity whose intent it recorded
        // cannot reach it.
        | TemporalStoreError::EntityIntent { .. } => FailureClass::InvariantViolation,
        TemporalStoreError::Refused(_)
        | TemporalStoreError::NotFound(_)
        | TemporalStoreError::TimestampPrecision(_)
        // A sweep never claims an instanceId, so this variant cannot reach
        // this classifier; it is grouped with the other client-input refusals
        // because that is what it would mean if it did.
        | TemporalStoreError::DuplicateInstanceClaim(_) => FailureClass::Permanent,
    };
    TaskFailure::new(class, error)
}
