//! A tenant over its snapshot quota evicts the snapshot first in order once its publication lock is released.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    problem::answered,
    ranked_snapshot::{ranked_id, ranked_snapshot},
    row_hold::RowHold,
    snapshot_capture::await_status,
    snapshot_lock::publication_lock,
    tenant_management::database_of,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    snapshot::{SettledStatus, SnapshotStatus},
};
use std::time::Duration;

/// How long the lowest ranked Snapshot's rows stay locked, shorter than the five seconds eviction waits for a held
/// Snapshot.
const HELD_FOR: Duration = Duration::from_secs(3);

/// Pins: On a broker holding at most three Snapshots per Tenant, with the control record and preparation row of the
/// Snapshot of `snapshotPriority` 1 held `FOR UPDATE` for 3 seconds, as the publication of its prepared branch holds
/// them, a fourth Snapshot evicts that Snapshot once the lock is released, and the Snapshots of `snapshotPriority` 5
/// and 10 and the fourth remain
/// Clause: 5.5.15, Table 5.2.41-1
/// Tenant: private (the quota counts every Snapshot of the Tenant)
#[gtest]
#[tokio::test]
async fn a_snp_lim_8141_a_tenant_over_its_snapshot_quota_evicts_the_snapshot_first_in_order_once_its_publication_lock_is_released() -> Result<()> {
    let namespace = Namespace::fresh();
    let durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&durable, &namespace).await?;
    let database = database_of(&durable, &namespace.tenant()?).await?;
    let low_id = ranked_id(&namespace, "low")?;
    let middle_id = ranked_id(&namespace, "middle")?;
    let high_id = ranked_id(&namespace, "high")?;
    let fourth_id = ranked_id(&namespace, "fourth")?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &low_id, 1, "PT10H".parse()?)?)
        .send()
        .await?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &middle_id, 5, "PT1H".parse()?)?)
        .send()
        .await?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &high_id, 10, "PT1H".parse()?)?)
        .send()
        .await?;
    // The hold stands for the low Snapshot's publication, so its own preparation has ended before the hold is taken.
    await_status(&tenant, &low_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    let hold = RowHold::seize(BrokerRole::Durable, &database, publication_lock(&low_id), HELD_FOR).await?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &fourth_id, 10, "PT2H".parse()?)?)
        .send()
        .await?;
    hold.release().await?;
    let evicted = tenant.with_responses().with_accept(Accept::any()).snapshots().retrieve(&low_id).send().await;
    expect_that!(&evicted, answered(StatusCode::NOT_FOUND));
    for kept in [&middle_id, &high_id, &fourth_id] {
        tenant.with_accept(Accept::any()).snapshots().retrieve(kept).send().await?;
    }
    for kept in [&middle_id, &high_id, &fourth_id] {
        tenant.snapshots().delete(kept).send().await?;
    }
    Ok(())
}
