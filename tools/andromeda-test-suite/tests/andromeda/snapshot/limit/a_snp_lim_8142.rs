//! A tenant over its snapshot quota ends the copy out of the snapshot first in order and evicts it.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    problem::problem,
    ranked_snapshot::{ranked_id, ranked_snapshot},
    slow_clone_source,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{SettledStatus, SnapshotClone, SnapshotStatus},
};
use std::time::Duration;
use tokio::time;

/// Pins: On a broker holding at most three Snapshots per Tenant, with a large source Snapshot of the default
/// `snapshotPriority` 5, a Snapshot of `snapshotPriority` 10 and a clone of the source of `snapshotPriority` 10 whose
/// copy is running, a fourth Snapshot of `snapshotPriority` 10 evicts the source, the clone ends `failure` as it does
/// when its source is deleted during the copy, and the other two Snapshots remain
/// Clause: 5.5.15, 5.16.2.4, Table 5.2.41-1, Table 5.2.41-2
/// Tenant: private (the quota counts every Snapshot of the Tenant, and the source is large)
#[gtest]
#[tokio::test]
async fn a_snp_lim_8142_a_tenant_over_its_snapshot_quota_ends_the_copy_out_of_the_snapshot_first_in_order_and_evicts_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::client(BrokerRole::Durable)?, &namespace).await?;
    let source_id = slow_clone_source::create(&tenant, &namespace).await?;
    let other_id = ranked_id(&namespace, "other")?;
    let clone_id = ranked_id(&namespace, "clone")?;
    let fourth_id = ranked_id(&namespace, "fourth")?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &other_id, 10, "PT1H".parse()?)?)
        .send()
        .await?;
    tenant
        .snapshots()
        .clone_snapshot(&source_id, &SnapshotClone::builder().id(clone_id.clone()).priority(10).build()?)
        .send()
        .await?;
    // A second after the clone is accepted its worker is copying the large source; the clone still reading
    // `preparing` right before the fourth create shows the copy has not finished.
    time::sleep(Duration::from_secs(1)).await;
    let preparing = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await?;
    expect_that!(preparing.status(), eq(SnapshotStatus::Preparing));
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &fourth_id, 10, "PT1H".parse()?)?)
        .send()
        .await?;
    let evicted = tenant.with_responses().snapshots().retrieve(&source_id).send().await;
    expect_that!(&evicted, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // Table 5.2.41-2: the clone's worker retries the copy, finds the source gone and settles the clone.
    await_status(&tenant, &clone_id, SnapshotStatus::Settled(SettledStatus::Failure)).await?;
    for kept in [&other_id, &fourth_id] {
        tenant.with_accept(Accept::any()).snapshots().retrieve(kept).send().await?;
    }
    for kept in [&clone_id, &other_id, &fourth_id] {
        tenant.snapshots().delete(kept).send().await?;
    }
    Ok(())
}
