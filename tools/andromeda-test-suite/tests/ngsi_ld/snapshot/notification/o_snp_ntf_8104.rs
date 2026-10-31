//! A refused snapshot notification is retried.

use andromeda_test_suite::{broker, eventually::retried, header_condition::HeaderCondition, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::headers::Accept,
    model::{KeyValuePair, NgsiLdMediaType},
    snapshot::{SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: A `SnapshotNotification` its endpoint answers 503 is sent again, with the same `receiverInfo` header and
/// `snapshotId`, and the Snapshot keeps its status and remains
/// Clause: 5.16.6
/// Recorded: clause 5.16.6 mandates when a `SnapshotNotification` is sent and says nothing of an endpoint refusing
/// it; Andromeda delivers it again after a backoff
#[gtest]
#[tokio::test]
async fn o_snp_ntf_8104_a_refused_snapshot_notification_is_retried() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/retry"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .with_priority(1),
    )
    .await?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .queries([])
        .endpoint(sink.endpoint("retry")?)
        .receiver_info([KeyValuePair::new("X-Retry-Proof", format!("yes-{namespace}"))])
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let retrieve = async || {
        broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await
    };
    retried(async || {
        let snapshot = retrieve().await?;
        verify_that!(snapshot.status(), eq(SnapshotStatus::Settled(SettledStatus::Failure)))?;
        Ok(())
    })
    .await?;
    let proof = [HeaderCondition::Is(HeaderName::from_static("x-retry-proof"), format!("yes-{namespace}"))];
    sink.wait_for_at_least_matching("retry", &[], &proof, 2).await?;
    let notifications = sink.received_matching("retry", &[], &proof).await?;
    expect_that!(notifications, len(eq(2)));
    for notification in &notifications {
        expect_that!(notification.snapshot()?.snapshot_id(), eq(&snapshot_id));
    }
    let snapshot_2 = retrieve().await?;
    expect_that!(snapshot_2.status(), eq(SnapshotStatus::Settled(SettledStatus::Failure)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
