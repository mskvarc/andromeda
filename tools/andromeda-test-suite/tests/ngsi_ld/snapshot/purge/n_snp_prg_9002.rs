//! Purge snapshots honours parentheses, a missing member, a data type mismatch and a string ordering.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::{StatusCode, header::CONTENT_LENGTH};
use ngsi_ld::{
    http::{headers::Accept, operation::PurgeSnapshots},
    model::{ErrorType, NgsiLdMediaType},
    query::q,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority},
};

/// Pins: Purge Snapshots evaluates a parenthesised association before the conjunction beside it, does not select a
/// Snapshot lacking `endpoint` by `endpoint!=`, answers 404 `ResourceNotFound` for a boolean compared with the Number
/// `snapshotPriority`, and selects by an ordering on `id` against a quoted string
/// Clause: 5.16.7.3, 5.16.7.4, 4.9, Table 5.2.41-1, Table 6.36.3.2-2
#[gtest]
#[tokio::test]
async fn n_snp_prg_9002_purge_snapshots_honours_parentheses_a_missing_member_a_data_type_mismatch_and_a_string_ordering() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoint = sink.endpoint("purge")?;
    let snapshot_ids = ["1", "2", "3", "4"].map(|suffix| format!("urn:ngsi-ld:Snapshot:{namespace}-{suffix}"));
    let [snapshot_1_id, snapshot_2_id, snapshot_3_id, snapshot_4_id] = snapshot_ids.each_ref().map(|id| id.parse::<SnapshotId>());
    let (snapshot_1_id, snapshot_2_id, snapshot_3_id, snapshot_4_id) = (snapshot_1_id?, snapshot_2_id?, snapshot_3_id?, snapshot_4_id?);
    for (id, priority) in [(&snapshot_1_id, 2), (&snapshot_2_id, 9), (&snapshot_3_id, 10)] {
        let snapshot = Snapshot::builder()
            .id(id.clone())
            .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
            .priority(priority)
            .endpoint(endpoint.clone())
            .build()?;
        broker.snapshots().create(&snapshot).send().await?;
    }
    let unnotified = Snapshot::builder()
        .id(snapshot_4_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .priority(9)
        .build()?;
    broker.snapshots().create(&unnotified).send().await?;
    let association = q::attr("snapshotPriority")
        .eq(2)
        .or(q::attr("snapshotPriority").eq(10))
        .and(q::attr("endpoint").eq(endpoint.as_str()))
        .build()?;
    let purged = broker.send_unchecked(broker.unchecked(&PurgeSnapshots { q: &association })?).await;
    expect_that!(&purged, answered(StatusCode::NO_CONTENT).without_header(CONTENT_LENGTH).without_body());
    for id in [&snapshot_1_id, &snapshot_3_id] {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    }
    for id in [&snapshot_2_id, &snapshot_4_id] {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::OK));
    }
    // Read as snapshotPriority==9|(snapshotPriority==2;id==...), the query would select -2 and -4 and answer 204.
    let only_purged = broker
        .with_responses()
        .snapshots()
        .purge(
            q::attr("snapshotPriority")
                .eq(9)
                .or(q::attr("snapshotPriority").eq(2))
                .and(q::attr("id").eq(snapshot_ids[0].as_str())),
        )
        .send()
        .await;
    expect_that!(&only_purged, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    for id in [&snapshot_2_id, &snapshot_4_id] {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::OK));
    }
    broker
        .snapshots()
        .purge(
            q::attr("endpoint")
                .ne("https://other.example.org/")
                .and(q::attr("snapshotPriority").eq(9))
                // The builder writes the pattern bare, where Andromeda admits only name characters.
                .and(q::attr("id").matches(format!("urn:ngsi-ld:Snapshot:{namespace}-"))),
        )
        .send()
        .await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_2_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    // A Snapshot without `endpoint` has no target element for `endpoint!=` (clause 4.9).
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_4_id)
        .send()
        .await?;
    // A boolean is never equal to the Number `snapshotPriority` (clause 4.9).
    let mismatched = broker
        .with_responses()
        .snapshots()
        .purge(q::attr("snapshotPriority").eq(true).and(q::attr("id").eq(snapshot_ids[3].as_str())))
        .send()
        .await;
    expect_that!(&mismatched, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_4_id)
        .send()
        .await?;
    expect_that!(remaining.priority(), some(eq(SnapshotPriority::try_new(9)?)));
    broker
        .snapshots()
        .purge(q::attr("id").ge(snapshot_ids[3].as_str()).and(q::attr("id").le(snapshot_ids[3].as_str())))
        .send()
        .await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_4_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
