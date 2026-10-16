//! The persistence a snapshot entity query writes its captured states through.
//!
//! A query's own failure is recorded in its execution result (clause 5.2.42).
//! A transient store failure -- a lost deadlock or serialization race, or a
//! saturated pool -- is not the query's: it is returned as an error instead, so
//! the whole preparation restarts rather than freezing a result the load of the
//! moment decided.

use crate::{
    error::{Recurrence, SnapshotError, entity_recurrence},
    executor::{
        SnapshotExecutor,
        execution_failure::ExecutionFailure,
        status::{failure_detail, success_detail},
    },
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    error::CommitError,
    submission_refusal::SubmissionRefusal,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::error::EntityError;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::ExecutionResultDetails;
use std::sync::Arc;
use tracing::error;

impl SnapshotExecutor {
    /// Write captured states into the Snapshot's branch as they are, each
    /// through the canonical codec (clause 5.16.1.4).
    pub(super) async fn persist_current_results(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        index: usize,
        states: Vec<EntityState>,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        if states.is_empty() {
            return Ok(success_detail(0));
        }
        let captured = states.len();
        let writes = states
            .into_iter()
            .map(|state| {
                let entity_id = Arc::new(state.id().clone()); // clone: the write owns its validated entity identity beside the state
                Ok(AtomicWrite::builder()
                    .entity_id(entity_id)
                    .entity(EntityMutation::Insert(EntityRecord::from_state(Arc::new(state))?))
                    .build())
            })
            .collect::<Result<Vec<AtomicWrite>, CommitError>>();
        let writes = match writes {
            Ok(writes) => writes,
            Err(error) => {
                error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to read captured Snapshot results");
                return Ok(failure_detail(ExecutionFailure::ReadResults { cause: &error }.to_string()));
            }
        };
        let outcomes = match self.writes.commit(target, writes).await {
            Ok(outcomes) => outcomes,
            Err(error) => {
                error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to materialize current Snapshot results");
                return branch_commit_failure(error);
            }
        };
        // A Snapshot captures the query's results whole, so one captured entity
        // the branch refused fails the query rather than leaving a capture that
        // silently lacks it.
        if let Some(refusal) = outcomes.into_iter().find_map(refusal_of) {
            error!(snapshot_id = %snapshot_id, query_index = index, %refusal, "A captured Snapshot result was refused");
            return branch_commit_failure(CommitError::Submission(refusal));
        }
        Ok(success_detail(captured))
    }
}

/// A failed local entity query: the query's own failure, or a transient store
/// failure the preparation restarts on.
pub(super) fn entity_query_failure(error: EntityError) -> Result<ExecutionResultDetails, SnapshotError> {
    match entity_recurrence(&error) {
        Recurrence::Transient => Err(SnapshotError::from(error)),
        Recurrence::Persistent => Ok(failure_detail(error.to_string())),
    }
}

/// The refusal a captured entity's write met, if the branch refused it.
fn refusal_of(outcome: WriteOutcome) -> Option<SubmissionRefusal> {
    match outcome {
        WriteOutcome::Refused(refusal) => Some(refusal),
        WriteOutcome::Created
        | WriteOutcome::Updated
        | WriteOutcome::Deleted(_)
        | WriteOutcome::AlreadyExists
        | WriteOutcome::NotFound
        | WriteOutcome::Superseded
        | WriteOutcome::Skipped
        | WriteOutcome::NoEntityChange => None,
    }
}

/// A failed write of captured entities into the staging branch: a transient
/// store failure the preparation restarts on, or the query's own failure.
fn branch_commit_failure(error: CommitError) -> Result<ExecutionResultDetails, SnapshotError> {
    match error {
        CommitError::TransactionContended => Err(SnapshotError::Contended),
        CommitError::StoreSaturated => Err(SnapshotError::StoreSaturated),
        CommitError::StoreUnavailable => Err(SnapshotError::StoreUnavailable),
        failure @ (CommitError::Submission { .. }
        | CommitError::DuplicateEntity { .. }
        | CommitError::Persistence { .. }
        | CommitError::Invariant { .. }
        | CommitError::ReplayLeaseLost { .. }
        | CommitError::UnexpectedOutcomeCount { .. }) => Ok(failure_detail(ExecutionFailure::StoreResults { cause: &failure }.to_string())),
    }
}
