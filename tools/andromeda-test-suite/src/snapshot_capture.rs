//! The wait for a Snapshot to settle: a Snapshot is created in the `preparing` status and settles once its queries
//! have run (clause 5.16.1.4), so a test that writes under it or reads it waits for the status it expects first.

use crate::{broker::BrokerClient, eventually::retried};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, ProblemDetails},
    snapshot::{ExecutionStatus, SnapshotId, SnapshotStatus},
};

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

/// Waits until the Snapshot `id`, of one entity query, reports `failure`, and answers the problem the execution result
/// of that query reports (clause 5.2.42).
///
/// # Errors
///
/// Returns the failure of the last read when the Snapshot never fails, or fails without one failed entity query
/// reporting a problem.
pub async fn await_query_problem(broker: &BrokerClient, id: &SnapshotId) -> Result<ProblemDetails> {
    retried(async || {
        let snapshot = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).snapshots().retrieve(id).send().await?;
        verify_that!(snapshot.status(), eq("failure".parse::<SnapshotStatus>()?))?;
        verify_that!(snapshot.output().queries_details, len(eq(1)))?;
        let details = snapshot.output().queries_details.first().or_fail()?;
        verify_that!(details.result_status, eq(ExecutionStatus::Failure))?;
        details.problem_details.clone().or_fail()
    })
    .await
}
