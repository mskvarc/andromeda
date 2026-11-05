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
    attempt_instant::AttemptInstant,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    error::CommitError,
    submission_refusal::SubmissionRefusal,
    write_outcome::WriteOutcome,
    write_retry::WRITE_ATTEMPTS,
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
    ///
    /// A captured entity whose Temporal Evolution the branch already holds at
    /// a later instant than the write's is stored again after that instant
    /// (clause 4.8), so the capture never silently lacks it.
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
        let states: Vec<Arc<EntityState>> = states.into_iter().map(Arc::new).collect();
        let mut pending: Vec<&Arc<EntityState>> = states.iter().collect();
        let mut instant = AttemptInstant::default();
        for _ in 0..WRITE_ATTEMPTS {
            let writes = match capture_writes(&pending) {
                Ok(writes) => writes,
                Err(error) => {
                    error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to read captured Snapshot results");
                    return Ok(failure_detail(ExecutionFailure::ReadResults { cause: &error }.to_string()));
                }
            };
            let outcomes = match self.writes.commit(target, instant.next(), writes).await {
                Ok(outcomes) => outcomes,
                Err(error) => {
                    error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to materialize current Snapshot results");
                    return branch_commit_failure(error);
                }
            };
            // A Snapshot captures the query's results whole, so one captured entity
            // the branch refused fails the query rather than leaving a capture that
            // silently lacks it.
            pending = match predated_captures(pending, outcomes, &mut instant) {
                Ok(predated) => predated,
                Err(refusal) => {
                    error!(snapshot_id = %snapshot_id, query_index = index, %refusal, "A captured Snapshot result was refused");
                    return branch_commit_failure(CommitError::Submission(refusal));
                }
            };
            if pending.is_empty() {
                return Ok(success_detail(captured));
            }
        }
        error!(snapshot_id = %snapshot_id, query_index = index, predated = pending.len(), "Captured Snapshot results stayed predated");
        Ok(failure_detail(ExecutionFailure::PredatedResults { count: pending.len() }.to_string()))
    }
}

/// The branch writes inserting each of `states` as it was captured.
fn capture_writes(states: &[&Arc<EntityState>]) -> Result<Vec<AtomicWrite>, CommitError> {
    states
        .iter()
        .map(|state| {
            let entity_id = Arc::new(state.id().clone()); // clone: the write owns its validated entity identity beside the state
            Ok(AtomicWrite::builder()
                .entity_id(entity_id)
                .entity(EntityMutation::Insert(EntityRecord::from_state(Arc::clone(state))?)) // clone: Arc ref-count bump; a predated capture is written again
                .build())
        })
        .collect()
}

/// The captured states whose write the branch refused as predated, each
/// latest instant recorded in `instant`, or the first refusal of a captured
/// entity's content.
fn predated_captures<'state>(
    pending: Vec<&'state Arc<EntityState>>,
    outcomes: Vec<WriteOutcome>,
    instant: &mut AttemptInstant,
) -> Result<Vec<&'state Arc<EntityState>>, SubmissionRefusal> {
    let mut predated = Vec::new();
    for (state, outcome) in pending.into_iter().zip(outcomes) {
        match outcome {
            WriteOutcome::Refused(refusal) => return Err(refusal),
            WriteOutcome::Predated(latest) => {
                instant.refused(latest);
                predated.push(state);
            }
            WriteOutcome::Created(_)
            | WriteOutcome::Updated(_)
            | WriteOutcome::Deleted(..)
            | WriteOutcome::AlreadyExists
            | WriteOutcome::NotFound
            | WriteOutcome::Superseded
            | WriteOutcome::Skipped
            | WriteOutcome::NoEntityChange => {}
        }
    }
    Ok(predated)
}

/// A failed local entity query: the query's own failure, or a transient store
/// failure the preparation restarts on.
pub(super) fn entity_query_failure(error: EntityError) -> Result<ExecutionResultDetails, SnapshotError> {
    match entity_recurrence(&error) {
        Recurrence::Transient => Err(SnapshotError::from(error)),
        Recurrence::Persistent => Ok(failure_detail(error.to_string())),
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
