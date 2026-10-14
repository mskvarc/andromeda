//! The reasons a Snapshot query or its preparation is recorded as failed.

use crate::error::SnapshotError;
use andromeda_context_commit::error::CommitError;
use andromeda_federation::error::FederationError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_temporal::error::TemporalError;
use thiserror::Error;

/// A failure that becomes the problem detail of an execution result
/// (clause 5.2.42), borrowing the typed error it reports.
///
/// The causes are borrowed for the moment the detail is rendered, so they are
/// not `source`s: an error source must be `'static`.
#[derive(Debug, Error)]
pub(super) enum ExecutionFailure<'error> {
    /// The preparation ended in a failure that recurs, so every query reports it.
    #[error("Snapshot preparation failed: {cause}")]
    Preparation {
        /// The failure that ended the preparation.
        cause: &'error SnapshotError,
    },
    /// The query translated from the stored Snapshot query failed validation.
    #[error("Snapshot query translation failed validation: {cause}")]
    QueryValidation {
        /// The parameter validation failure.
        cause: &'error ParameterError,
    },
    /// No local entity matched and the Context Source forwarding could not run.
    #[error("no valid entity result was available because distributed execution failed: {cause}")]
    DistributedExecution {
        /// The forwarding failure.
        cause: &'error FederationError,
    },
    /// The captured states could not be turned into branch writes.
    #[error("failed to read results: {cause}")]
    ReadResults {
        /// The failure encoding a captured state.
        cause: &'error CommitError,
    },
    /// The branch could not store the captured entities.
    #[error("failed to store results: {cause}")]
    StoreResults {
        /// The failure committing to the branch.
        cause: &'error CommitError,
    },
    /// The branch could not store the captured temporal evolutions.
    #[error("failed to store temporal results: {cause}")]
    StoreTemporalResults {
        /// The failure recording the history.
        cause: &'error TemporalError,
    },
}
