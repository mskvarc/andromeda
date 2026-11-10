//! The wait for a Snapshot to settle: a Snapshot is created in the `preparing` status and settles once its queries
//! have run (clause 5.16.1.4), so a test that writes under it or reads it waits for the status it expects first.

use crate::{broker::BrokerClient, eventually::retried};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, ProblemDetails},
    snapshot::{ExecutionResultDetails, ExecutionStatus, SettledStatus, SnapshotId, SnapshotOutputMembers, SnapshotStatus},
};

/// A query of a Snapshot, by the list it is in and its index there; its execution result has the same index in the
/// details of that list (clause 5.16.1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotQuery {
    /// The entry of `snapshotQueries`, whose results are `snapshotQueriesDetails`, at the index.
    Entity(usize),
    /// The entry of `snapshotTemporalQueries`, whose results are `snapshotTemporalQueriesDetails`, at the index.
    Temporal(usize),
}

impl SnapshotQuery {
    /// The execution result of this query, `None` when the list holds none at its index.
    fn detail(self, output: &SnapshotOutputMembers) -> Option<&ExecutionResultDetails> {
        match self {
            SnapshotQuery::Entity(index) => output.queries_details.get(index),
            SnapshotQuery::Temporal(index) => output.temporal_queries_details.get(index),
        }
    }
}

/// Waits until the Snapshot `id` reports `status`, reading it every poll interval up to the delivery deadline.
///
/// # Errors
///
/// Returns the failure of the last read when the Snapshot never reports `status`.
pub async fn await_status(broker: &BrokerClient, id: &SnapshotId, status: SnapshotStatus) -> Result<()> {
    retried(async || {
        let snapshot = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).snapshots().retrieve(id).send().await?;
        verify_that!(snapshot.status(), eq(status))?;
        Ok(())
    })
    .await
}

/// Waits until the Snapshot `id` reports `settled`, and answers the problem the execution result of `query` reports
/// (clause 5.2.42).
///
/// # Errors
///
/// Returns the failure of the last read when the Snapshot never settles so, or settles without `query` failing with a
/// problem.
pub async fn await_query_problem(broker: &BrokerClient, id: &SnapshotId, settled: SettledStatus, query: SnapshotQuery) -> Result<ProblemDetails> {
    retried(async || {
        let snapshot = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).snapshots().retrieve(id).send().await?;
        verify_that!(snapshot.status(), eq(SnapshotStatus::Settled(settled)))?;
        let detail = query.detail(snapshot.output()).or_fail()?;
        verify_that!(detail.result_status, eq(ExecutionStatus::Failure))?;
        detail.problem_details.clone().or_fail()
    })
    .await
}
