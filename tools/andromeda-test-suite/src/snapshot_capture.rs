//! The wait for a Snapshot to settle: a Snapshot is created in the `preparing` status and settles once its queries
//! have run (clause 5.16.1.4), so a test that writes under it or reads it waits for the status it expects first.

use crate::{broker::BrokerClient, eventually::retried};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, ProblemDetails},
    snapshot::{ExecutionResultDetails, ExecutionStatus, SettledStatus, SnapshotId, SnapshotOutputMembers, SnapshotStatus},
};

/// The list of queries of a Snapshot an execution result belongs to (clause 5.16.1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryList {
    /// `snapshotQueries`, whose results are `snapshotQueriesDetails`.
    Entity,
    /// `snapshotTemporalQueries`, whose results are `snapshotTemporalQueriesDetails`.
    Temporal,
}

impl QueryList {
    /// The execution results of the queries of this list.
    fn details(self, output: &SnapshotOutputMembers) -> &[ExecutionResultDetails] {
        match self {
            QueryList::Entity => &output.queries_details,
            QueryList::Temporal => &output.temporal_queries_details,
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

/// Waits until the Snapshot `id` reports `settled`, with one query in `list`, and answers the problem the execution
/// result of that query reports (clause 5.2.42).
///
/// # Errors
///
/// Returns the failure of the last read when the Snapshot never settles so, or settles without the one query of `list`
/// failing with a problem.
pub async fn await_query_problem(broker: &BrokerClient, id: &SnapshotId, settled: SettledStatus, list: QueryList) -> Result<ProblemDetails> {
    retried(async || {
        let snapshot = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).snapshots().retrieve(id).send().await?;
        verify_that!(snapshot.status(), eq(SnapshotStatus::Settled(settled)))?;
        let details = list.details(snapshot.output());
        verify_that!(details, len(eq(1)))?;
        let details = details.first().or_fail()?;
        verify_that!(details.result_status, eq(ExecutionStatus::Failure))?;
        details.problem_details.clone().or_fail()
    })
    .await
}
