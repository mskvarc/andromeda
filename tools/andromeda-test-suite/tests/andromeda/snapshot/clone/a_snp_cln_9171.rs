//! A delete of a clone's source during the copy returns within the request timeout and fails the clone.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, slow_clone_source, snapshot_capture::await_status};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{SettledStatus, SnapshotClone, SnapshotId, SnapshotStatus},
};
use std::time::{Duration, Instant};
use tokio::time;

/// Pins: Delete Snapshot of the source of a clone whose copy is running answers 204 in under 5 seconds, well within
/// the 30 second `http.request_timeout`, and the clone ends `failure`
/// Clause: 5.16.2.4, 5.16.5, Table 5.2.41-2
/// Tenant: private (the source is large, and no other test's Snapshots share its tenant)
#[gtest]
#[tokio::test]
async fn a_snp_cln_9171_a_delete_of_a_clones_source_during_the_copy_returns_within_the_request_timeout_and_fails_the_clone() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source_id = slow_clone_source::create(&tenant, &namespace).await?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-clone").parse::<SnapshotId>()?;
    tenant
        .snapshots()
        .clone_snapshot(&source_id, &SnapshotClone::builder().id(clone_id.clone()).build()?)
        .send()
        .await?;
    let json = tenant.with_accept(Accept::new(NgsiLdMediaType::Json));
    // A second after the clone is accepted its worker is copying the large source; the clone still reading
    // `preparing` right before the delete shows the copy has not finished.
    time::sleep(Duration::from_secs(1)).await;
    let preparing = json.snapshots().retrieve(&clone_id).send().await?;
    expect_that!(preparing.status(), eq(SnapshotStatus::Preparing));
    let started = Instant::now();
    tenant.snapshots().delete(&source_id).send().await?;
    expect_that!(started.elapsed(), lt(Duration::from_secs(5)));
    let gone = tenant.with_responses().snapshots().retrieve(&source_id).send().await;
    expect_that!(&gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // Table 5.2.41-2: the clone's worker retries the copy, finds the source gone and settles the clone.
    await_status(&tenant, &clone_id, SnapshotStatus::Settled(SettledStatus::Failure)).await?;
    tenant.snapshots().delete(&clone_id).send().await?;
    Ok(())
}
