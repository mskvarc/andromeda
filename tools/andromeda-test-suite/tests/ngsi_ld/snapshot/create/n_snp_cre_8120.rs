//! Snapshot members are held to their restrictions.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Duration, ErrorType, KeyValuePair, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority},
};

/// Posts `body` as written to Create Snapshot and expects `BadRequestData`.
async fn expect_refused(broker: &BrokerClient, body: &str) -> Result<()> {
    // The bodies break the Snapshot data type, which the typed builders refuse, so each is written as text.
    let request = json_request_to(broker, Method::POST, "snapshots", body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}

/// Asserts that no Snapshot `id` exists.
async fn expect_absent(broker: &BrokerClient, id: &SnapshotId) {
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
}

/// Pins: Create Snapshot accepts `snapshotPriority` 1 and 10 with an endpoint and `receiverInfo`, sets
/// `snapshotPriority` 5 when it is absent, and answers 400 `BadRequestData` for `snapshotPriority` 0, 11 and 1.5, a
/// `snapshotLifetime` that is no duration, an endpoint that is no URI and a `receiverInfo` that is no
/// `KeyValuePair`
/// Clause: 5.16.1.4, Table 5.2.41-1, 5.2.22
#[gtest]
#[tokio::test]
async fn n_snp_cre_8120_snapshot_members_are_held_to_their_restrictions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let query = format!(r#"{{"type": "Query", "entities": [{{"type": "{vehicle_type}"}}]}}"#);
    let sink = NotificationSink::start().await?;
    let low_id = format!("urn:ngsi-ld:Snapshot:{namespace}-low").parse::<SnapshotId>()?;
    let high_id = format!("urn:ngsi-ld:Snapshot:{namespace}-high").parse::<SnapshotId>()?;
    let default_id = format!("urn:ngsi-ld:Snapshot:{namespace}-default").parse::<SnapshotId>()?;
    for (id, priority, label) in [(&low_id, 1, "low"), (&high_id, 10, "high")] {
        let snapshot = Snapshot::builder()
            .id(id.clone())
            .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
            .lifetime("PT30M".parse::<Duration>()?)
            .priority(priority)
            .endpoint(sink.endpoint(label)?.as_str())
            .receiver_info([KeyValuePair::new("X-Correlation", label)])
            .build()?;
        broker.snapshots().create(&snapshot).send().await?;
        let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).snapshots().retrieve(id).send().await?;
        expect_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(priority)?)));
    }
    let members = [
        r#""snapshotPriority": 0"#,
        r#""snapshotPriority": 11"#,
        r#""snapshotPriority": 1.5"#,
        r#""snapshotLifetime": "one hour""#,
        r#""endpoint": "not a URL""#,
        r#""receiverInfo": [{"key": 4, "value": []}]"#,
    ];
    for (index, member) in members.iter().enumerate() {
        let id = format!("urn:ngsi-ld:Snapshot:{namespace}-bad-{index}");
        expect_refused(
            &broker,
            &format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": [{query}], {member}}}"#),
        )
        .await?;
    }
    expect_absent(&broker, &format!("urn:ngsi-ld:Snapshot:{namespace}-bad-0").parse::<SnapshotId>()?).await;
    let default = Snapshot::builder()
        .id(default_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&default).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&default_id)
        .send()
        .await?;
    expect_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(5)?)));
    broker.snapshots().delete(&default_id).send().await?;
    broker.snapshots().delete(&high_id).send().await?;
    broker.snapshots().delete(&low_id).send().await?;
    Ok(())
}
