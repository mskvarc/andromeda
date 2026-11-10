//! The persistence a snapshot entity query writes its captured states through.
//!
//! A query's own failure is recorded in its execution result (clause 5.2.42),
//! a failed local read as the API reports the same failure.
//! A transient store failure (a lost deadlock or serialization race, or a
//! saturated pool) is not the query's: it is returned as an error instead, so
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
use andromeda_failure_classification::entity_failure::classify_entity_error;
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
                    return Ok(failure_detail(ExecutionFailure::ReadResults { cause: &error }.problem()));
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
        Ok(failure_detail(ExecutionFailure::PredatedResults { count: pending.len() }.problem()))
    }
}

/// The branch writes inserting each of `states` as it was captured.
fn capture_writes(states: &[&Arc<EntityState>]) -> Result<Vec<AtomicWrite>, CommitError> {
    states
        .iter()
        .map(|state| {
            let entity_id = Arc::new(state.id().clone()); // clone: the write owns its entity identity beside the state it records
            Ok(AtomicWrite::builder()
                .entity_id(entity_id)
                .entity(EntityMutation::Insert(EntityRecord::from_state(Arc::clone(state))?))
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
        Recurrence::Persistent => Ok(failure_detail(classify_entity_error(&error).into_problem())),
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
        | CommitError::UnexpectedOutcomeCount { .. }) => Ok(failure_detail(ExecutionFailure::StoreResults { cause: &failure }.problem())),
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::execution::{branch_commit_failure, entity_query_failure};
    use andromeda_context_commit::error::CommitError;
    use andromeda_entity::error::EntityError;
    use andromeda_ngsi_ld_model::snapshot::ExecutionResultDetails;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_ngsi_ld_query::error::QueryError;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;

    /// The classification and detail a failed query records.
    fn recorded(detail: &ExecutionResultDetails) -> Option<(ProblemClassification, &str)> {
        detail.problem_details.as_ref().map(|problem| (problem.classification(), problem.detail()))
    }

    /// Clause 5.16.1.4 executes the query following clause 5.7.2.4: a local
    /// read that fails for a reason of its own records the type Query Entities
    /// answers it with, and an internal one its masked summary.
    #[test]
    fn a_failed_local_read_records_the_problem_query_entities_answers_with() {
        let query = entity_query_failure(EntityError::Query(QueryError::QEmpty)).expect("the query's own failure");
        assert_eq!(
            recorded(&query).map(|(classification, _)| classification),
            Some(ProblemClassification::Category(NgsiLdErrorType::BadRequestData))
        );
        let store = entity_query_failure(EntityError::Persistence(PersistenceFailure::StatementRejected)).expect("a fault no restart mends");
        assert_eq!(
            recorded(&store),
            Some((
                ProblemClassification::Category(NgsiLdErrorType::InternalError),
                "the entity store could not complete the operation"
            ))
        );
    }

    /// A transient store failure is not the query's: the preparation restarts.
    #[test]
    fn a_transient_local_read_failure_restarts_the_preparation() {
        for error in [EntityError::StoreSaturated, EntityError::StoreUnavailable, EntityError::TransactionContended] {
            assert!(entity_query_failure(error).is_err());
        }
    }

    /// Writing into the Snapshot is the capture's own step, so its fault is an
    /// `InternalError`, and a transient one restarts the preparation.
    #[test]
    fn a_failed_branch_write_is_an_internal_error_unless_it_is_transient() {
        let failed = branch_commit_failure(CommitError::Persistence(PersistenceFailure::StatementRejected)).expect("a fault no restart mends");
        assert_eq!(
            recorded(&failed).map(|(classification, _)| classification),
            Some(ProblemClassification::Category(NgsiLdErrorType::InternalError))
        );
        for error in [CommitError::TransactionContended, CommitError::StoreSaturated, CommitError::StoreUnavailable] {
            assert!(branch_commit_failure(error).is_err());
        }
    }
}
