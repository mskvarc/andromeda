//! A delete of a clone during its copy returns within the request timeout.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, slow_clone_source};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{SettledStatus, SnapshotClone, SnapshotId, SnapshotStatus},
};
use std::time::{Duration, Instant};
use tokio::time;

/// Pins: Delete Snapshot of a clone whose copy is running answers 204 in under 5 seconds, well within the 30 second
/// `http.request_timeout`, the clone stays gone, and its source keeps `success`
/// Clause: 5.16.2.4, 5.16.5
/// Tenant: private (the source is large, and no other test's Snapshots share its tenant)
#[gtest]
#[tokio::test]
async fn a_snp_cln_9170_a_delete_of_a_clone_during_its_copy_returns_within_the_request_timeout() -> Result<()> {
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
    tenant.snapshots().delete(&clone_id).send().await?;
    expect_that!(started.elapsed(), lt(Duration::from_secs(5)));
    let gone = tenant.with_responses().snapshots().retrieve(&clone_id).send().await;
    expect_that!(&gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The clone's worker finds the clone gone when it restarts its claim, and its few retries are milliseconds
    // apart, so it has ended and recorded nothing well before three seconds pass: the clone does not come back.
    time::sleep(Duration::from_secs(3)).await;
    let still_gone = tenant.with_responses().snapshots().retrieve(&clone_id).send().await;
    expect_that!(&still_gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let source = json.snapshots().retrieve(&source_id).send().await?;
    expect_that!(source.status(), eq(SnapshotStatus::Settled(SettledStatus::Success)));
    tenant.snapshots().delete(&source_id).send().await?;
    Ok(())
}
