//! Recording a failed temporal step of a snapshot query (clause 5.2.42).
//!
//! A query's own failure is recorded in its execution result. A transient
//! store failure is not the query's: it is returned as an error instead, so
//! the whole preparation restarts.

use crate::{
    error::{Recurrence, SnapshotError, temporal_recurrence},
    executor::{execution_failure::ExecutionFailure, status::failure_detail},
};
use andromeda_ngsi_ld_model::snapshot::ExecutionResultDetails;
use andromeda_temporal::error::TemporalError;

/// Which temporal operation of a snapshot query failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporalStep {
    /// Reading a page of temporal entities.
    Query,
    /// Writing a captured evolution into the branch.
    Store,
}

/// A failed temporal query or temporal write, `step` naming which: the
/// query's own failure, or a transient store failure the preparation restarts
/// on.
pub(crate) fn temporal_failure(error: TemporalError, step: TemporalStep) -> Result<ExecutionResultDetails, SnapshotError> {
    match temporal_recurrence(&error) {
        Recurrence::Transient => Err(SnapshotError::from(error)),
        Recurrence::Persistent => Ok(failure_detail(match step {
            TemporalStep::Query => error.to_string(),
            TemporalStep::Store => ExecutionFailure::StoreTemporalResults { cause: &error }.to_string(),
        })),
    }
}
