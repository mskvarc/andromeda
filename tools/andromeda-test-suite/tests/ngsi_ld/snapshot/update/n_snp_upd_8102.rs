//! An updated lifetime sets the expiry from the update.

use andromeda_test_suite::{broker, namespace::Namespace, snapshot_capture::await_status};
use chrono::Utc;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotFragment, SnapshotId, SnapshotStatus},
};

/// Whole days from now to `expires_at`.
fn days_after_now(expires_at: &DateTime) -> i64 {
    (expires_at.timestamp().as_second() - Utc::now().timestamp()) / 86_400
}

/// Pins: Update Snapshot Status with `snapshotLifetime` P10D on a Snapshot of the default lifetime answers 200 with
/// `expiresAt` ten days from the update (nine whole days from now)
/// Clause: 5.16.4.4, Table 5.2.41-1, Table 5.2.41-2
#[gtest]
#[tokio::test]
async fn n_snp_upd_8102_an_updated_lifetime_sets_the_expiry_from_the_update() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    let before = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(days_after_now(before.expires_at()), eq(0));
    let fragment = SnapshotFragment::builder().lifetime("P10D".parse()?).build()?;
    let updated = broker.snapshots().update(&snapshot_id, &fragment).send().await?;
    expect_that!(days_after_now(updated.expires_at()), ge(9));
    expect_that!(days_after_now(updated.expires_at()), le(10));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
