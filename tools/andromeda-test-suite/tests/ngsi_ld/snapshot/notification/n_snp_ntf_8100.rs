//! A snapshot with an endpoint is notified of its terminal status.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    header_condition::HeaderCondition,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityTypeName, KeyValuePair, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority, SnapshotStatus},
};

/// Pins: A Snapshot with an endpoint and `receiverInfo` sends one `SnapshotNotification` once it ends success,
/// empty, partial (a query with a result beside one without) or failure (no query to execute), carrying the
/// `receiverInfo` as a header, its `snapshotId`, `snapshotStatus`, `snapshotPriority` and `expiresAt`, and a
/// `notifiedAt`
/// Clause: 5.16.6, 5.3.4, 5.16.1.4
#[gtest]
#[tokio::test]
async fn n_snp_ntf_8100_a_snapshot_with_an_endpoint_is_notified_of_its_terminal_status() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let res_type = namespace.entity_type("Res")?;
    let absent_type = namespace.entity_type("Absent")?;
    let res = Entity::builder()
        .id(namespace.urn("Res", 1)?)
        .entity_type(res_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&res).send().await?.done()?;
    let selection = |type_name: &EntityTypeName| QueryDocument::builder().entities([type_name.as_str()]).build();
    let cases = [
        ("success", vec![selection(&res_type)?], "success"),
        ("empty", vec![selection(&absent_type)?], "empty"),
        ("partial", vec![selection(&res_type)?, selection(&absent_type)?], "partial"),
        ("failure", vec![], "failure"),
    ];
    let mut ids = Vec::new();
    for (label, queries, status) in cases {
        let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-{label}").parse::<SnapshotId>()?;
        let snapshot = Snapshot::builder()
            .id(snapshot_id.clone())
            .queries(queries)
            .endpoint(sink.endpoint(label)?.as_str())
            .receiver_info([KeyValuePair::new("X-Snapshot-Test", format!("{label}-{namespace}"))])
            .build()?;
        broker.snapshots().create(&snapshot).send().await?;
        let expires_at = retried(async || {
            let retrieved = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .snapshots()
                .retrieve(&snapshot_id)
                .send()
                .await?;
            verify_that!(retrieved.status(), eq(status.parse::<SnapshotStatus>()?))?;
            Ok(retrieved.output().expires_at)
        })
        .await?;
        let header = [HeaderCondition::exactly(
            HeaderName::from_static("x-snapshot-test"),
            &format!("^{label}-{namespace}$"),
        )?];
        let notifications = sink.wait_for_at_least_matching(label, &[], &header, 1).await?;
        expect_that!(notifications, len(eq(1)));
        let notification = notifications.first().or_fail()?.snapshot()?;
        expect_that!(notification.snapshot_id(), eq(&snapshot_id));
        expect_that!(notification.status().to_string(), eq(status));
        expect_that!(notification.priority(), eq(SnapshotPriority::try_new(5)?));
        expect_that!(notification.expires_at(), eq(&expires_at));
        expect_that!(notification.id().to_string(), not(eq("")));
        ids.push(snapshot_id);
    }
    for snapshot_id in &ids {
        broker.snapshots().delete(snapshot_id).send().await?;
    }
    Ok(())
}
