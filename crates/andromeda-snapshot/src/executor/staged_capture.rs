//! Capturing every query of a Snapshot into its staging branch, starting over
//! without a query that failed after it staged part of its result (clause
//! 5.16.1.4).
//!
//! Clause 5.16.1.4 stores the Entity information each query retrieved, and a
//! query that fails retrieves none. When one fails after it wrote into the
//! staging branch, the branch is emptied and every query is captured again in
//! order, that one held at the failure it met, so the branch ends holding
//! exactly what the queries that did not fail captured, each entity as they
//! captured it. Each round holds one more query back, so the capture ends after
//! at most one round per query.

use crate::{
    error::SnapshotError,
    executor::{SnapshotExecutor, attempts::restart_staging, query_capture::QueryCapture, status::ExecutionAggregate},
    snapshot_invariant::SnapshotInvariant,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, Snapshot};
use std::collections::HashSet;
use tracing::warn;

/// A query of a Snapshot, by the list it is in and its index there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryPosition {
    /// The entry of `snapshotQueries` at the index.
    Entity(usize),
    /// The entry of `snapshotTemporalQueries` at the index.
    Temporal(usize),
}

/// The queries that failed after staging part of their result, each held at
/// the failure it met.
#[derive(Debug, Default)]
struct HeldFailures(Vec<(QueryPosition, ExecutionResultDetails)>);

impl HeldFailures {
    /// The failure the query at `position` is held at, `None` when it is to
    /// be captured.
    fn failure_of(&self, position: QueryPosition) -> Option<ExecutionResultDetails> {
        self.0
            .iter()
            .find(|(held, _)| *held == position)
            // clone: every later round holds the query at the same failure
            .map(|(_, detail)| detail.clone())
    }

    /// Hold the query at `position` at `failure` in every later round.
    fn hold(&mut self, position: QueryPosition, failure: ExecutionResultDetails) {
        self.0.push((position, failure));
    }
}

/// How one round of capturing every query into an empty staging branch ended.
enum CaptureRound {
    /// Every query was captured or held, and the branch holds what those that
    /// did not fail captured.
    Complete(ExecutionAggregate),
    /// The query at the position failed after staging part of its result.
    Spoiled(QueryPosition, ExecutionResultDetails),
}

impl SnapshotExecutor {
    /// Capture every query of `snapshot` into its staging branch `target`, and
    /// the execution result of each once the branch holds only what the
    /// queries that did not fail captured.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure a query or a staging write met, the
    /// store's failure when the staging branch could not be emptied, and
    /// [`SnapshotInvariant::ClaimLost`] when `claimed_by` no longer held the
    /// preparation when it was to be.
    pub(super) async fn capture_queries(
        &self,
        snapshot: &Snapshot,
        target: &ContextTarget,
        context: &ContextSpec,
        claimed_by: LeaseHolder,
    ) -> Result<ExecutionAggregate, SnapshotError> {
        let mut held = HeldFailures::default();
        loop {
            match self.capture_round(snapshot, target, context, &held).await? {
                CaptureRound::Complete(aggregate) => return Ok(aggregate),
                CaptureRound::Spoiled(position, failure) => {
                    warn!(snapshot_id = %snapshot.id, query = ?position, "Snapshot query failed after staging part of its result; capturing every query again without it");
                    held.hold(position, failure);
                    if !restart_staging(self.snapshot_repo.as_ref(), &snapshot.id, claimed_by, self.lease).await? {
                        return Err(SnapshotInvariant::ClaimLost {
                            snapshot: snapshot.id.clone(), // clone: the error owns the Snapshot it names.
                        }
                        .into());
                    }
                }
            }
        }
    }

    /// Capture, in order, every query of `snapshot` that `held` does not hold
    /// into an empty staging branch, ending at the first that fails after
    /// staging part of its result.
    async fn capture_round(
        &self,
        snapshot: &Snapshot,
        target: &ContextTarget,
        context: &ContextSpec,
        held: &HeldFailures,
    ) -> Result<CaptureRound, SnapshotError> {
        let mut aggregate = ExecutionAggregate::default();
        let mut materialized_entities = HashSet::new();
        for (index, query) in snapshot.snapshot_queries.iter().flatten().enumerate() {
            let position = QueryPosition::Entity(index);
            let detail = match held.failure_of(position) {
                Some(failure) => failure,
                // clone: every round translates the stored query again.
                None => match self
                    .run_entity_query(target, &snapshot.id, query.clone(), index, context, &mut materialized_entities)
                    .await?
                {
                    QueryCapture::Settled(detail) => detail,
                    QueryCapture::FailedAfterStaging(failure) => return Ok(CaptureRound::Spoiled(position, failure)),
                },
            };
            aggregate.push_entity(detail);
        }
        for (index, query) in snapshot.snapshot_temporal_queries.iter().flatten().enumerate() {
            let position = QueryPosition::Temporal(index);
            let detail = match held.failure_of(position) {
                Some(failure) => failure,
                // clone: every round translates the stored query again.
                None => match self.run_temporal_query(target, &snapshot.id, query.clone(), index, context).await? {
                    QueryCapture::Settled(detail) => detail,
                    QueryCapture::FailedAfterStaging(failure) => return Ok(CaptureRound::Spoiled(position, failure)),
                },
            };
            aggregate.push_temporal(detail);
        }
        Ok(CaptureRound::Complete(aggregate))
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::{
        execution_failure::ExecutionFailure,
        staged_capture::{HeldFailures, QueryPosition},
        status::failure_detail,
    };
    use andromeda_ngsi_ld_model::snapshot::ExecutionResultStatus;

    #[test]
    fn a_held_query_keeps_the_failure_it_met() {
        let mut held = HeldFailures::default();
        held.hold(QueryPosition::Entity(1), failure_detail(ExecutionFailure::OffsetOverflow.problem()));

        let failure = held.failure_of(QueryPosition::Entity(1)).expect("a held query");

        pretty_assertions::assert_eq!(failure.result_status, ExecutionResultStatus::Failure);
        pretty_assertions::assert_eq!(
            failure.problem_details.map(|problem| problem.detail().to_owned()),
            Some(ExecutionFailure::OffsetOverflow.to_string())
        );
    }

    /// An entity query and a temporal query at the same index are two
    /// queries: holding one leaves the other to be captured.
    #[test]
    fn holding_a_query_leaves_every_other_to_be_captured() {
        let mut held = HeldFailures::default();
        held.hold(QueryPosition::Temporal(0), failure_detail(ExecutionFailure::OffsetOverflow.problem()));

        assert!(held.failure_of(QueryPosition::Entity(0)).is_none());
        assert!(held.failure_of(QueryPosition::Temporal(1)).is_none());
        assert!(held.failure_of(QueryPosition::Temporal(0)).is_some());
    }
}
