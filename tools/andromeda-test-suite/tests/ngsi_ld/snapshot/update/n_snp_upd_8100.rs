//! Update snapshot status merges its fragment and removes members written as the ngsi ld null.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Duration, KeyValuePair, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotFragment, SnapshotId, SnapshotMember, SnapshotPriority},
};

/// Pins: Update Snapshot Status with `snapshotLifetime`, `snapshotPriority`, endpoint and `receiverInfo` answers
/// 200 with the Snapshot carrying them, and a second update setting endpoint and `receiverInfo` to the NGSI-LD Null
/// answers it without them and with the `snapshotPriority` kept
/// Clause: 5.16.4.4, 5.5.8, 4.5.0, 6.37.3.2
#[gtest]
#[tokio::test]
async fn n_snp_upd_8100_update_snapshot_status_merges_its_fragment_and_removes_members_written_as_the_ngsi_ld_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint("old")?)
        .receiver_info([KeyValuePair::new("X-Old", "yes")])
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let fragment = SnapshotFragment::builder()
        .lifetime("PT2H".parse()?)
        .priority(7)
        .endpoint(sink.endpoint("new")?)
        .receiver_info([KeyValuePair::new("X-New", "yes")])
        .build()?;
    let updated = broker.snapshots().update(&snapshot_id, &fragment).send().await?;
    expect_that!(updated.priority(), some(eq(SnapshotPriority::try_new(7)?)));
    expect_that!(updated.lifetime(), some(eq(&"PT2H".parse::<Duration>()?)));
    expect_that!(updated.endpoint(), some(eq(&sink.endpoint("new")?)));
    expect_that!(updated.receiver_info(), len(eq(1)));
    expect_that!(updated.receiver_info().first().map(KeyValuePair::key), some(eq("X-New")));
    let removal = SnapshotFragment::builder()
        .delete(SnapshotMember::Endpoint)
        .delete(SnapshotMember::ReceiverInfo)
        .build()?;
    let updated_2 = broker.snapshots().update(&snapshot_id, &removal).send().await?;
    expect_that!(updated_2.endpoint(), none());
    expect_that!(updated_2.receiver_info(), is_empty());
    expect_that!(updated_2.priority(), some(eq(SnapshotPriority::try_new(7)?)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(retrieved.endpoint(), none());
    expect_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(7)?)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
